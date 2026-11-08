#!/usr/bin/env node
/**
 * jev-desktop: one goal in, a desktop driven to it.
 *
 *   node scripts/jev/run.mjs --app Finder "open the Applications folder"
 *
 * The caller states a goal once. Each turn reads the live accessibility tree,
 * asks for one operation and a target for that operation, carries it out, and
 * reads again. The tree never reaches the caller.
 */
import { fileURLToPath } from "node:url";

import { setTimeout as sleep } from "node:timers/promises";

import { describe, windowSuccessor } from "./screen.mjs";
import { clipboardGuard, execute, listWindows, observe, startCursor, stopCursor } from "./desktop.mjs";
import {
  TERMINALS,
  typesafeApi,
  actionSpace,
  buildRequest,
  criterion,
  fingerprint,
  needsRiskCheck,
  riskRequest,
  route,
  shouldStop,
  textSupply,
  validateChoice,
} from "./policy.mjs";


const post = async (url, key, body) => {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const res = await fetch(url, {
      method: "POST",
      headers: { Authorization: `Bearer ${key}`, "Content-Type": "application/json" },
      body: JSON.stringify(body),
      redirect: "error",
    }).catch(() => null);
    if (!res) throw new Error("the model could not be reached; nothing ran");
    if ([429, 503, 529].includes(res.status) && attempt < 2) {
      await new Promise((r) => setTimeout(r, 500 * 2 ** attempt));
      continue;
    }
    if (!res.ok) {
      throw new Error(`the model returned HTTP ${res.status}; nothing ran`);
    }
    try {
      return await res.json();
    } catch {
      throw new Error("the model returned an invalid response; nothing ran");
    }
  }
  throw new Error("the model stayed unavailable");
};

export const decide = (goal, screen, space, history, values) =>
  post(typesafeApi(), process.env.TYPESAFE_API_KEY, buildRequest(goal, screen, space, history, { values }));

export const rateRisk = (goal, screen, operation, node, values) =>
  post(typesafeApi(), process.env.TYPESAFE_API_KEY, riskRequest(goal, screen, operation, node, { values }));

const WINDOW_STOPS = {
  WINDOW_NOT_FOUND: "window_closed",
  SURFACE_OUTSIDE_WINDOW: "surface_outside_window",
  ROOT_OUTSIDE_WINDOW: "root_outside_window",
};

const redactInputs = (value, inputs) => {
  if (value == null || inputs.size === 0) return value;
  const secrets = [...inputs].sort((a, b) => b.length - a.length);
  return JSON.parse(JSON.stringify(value, (_key, item) => {
    if (typeof item !== "string") return item;
    return secrets.reduce((text, secret) => text.replaceAll(secret, "[redacted]"), item);
  }));
};

const windowStop = (error, windowId) => (windowId ? WINDOW_STOPS[error.code] ?? null : null);

/** How long a pinned run waits for a replacement window after its window goes. */
const followWindowMs = (raw) => {
  const value = Number(raw ?? 3000);
  if (!Number.isFinite(value) || value < 0 || value > 60000) {
    throw new Error(`JEV_WINDOW_FOLLOW_MS must be a number of milliseconds from 0 to 60000, not ${JSON.stringify(raw)}`);
  }
  return value;
};
const FOLLOW_MS = followWindowMs(process.env.JEV_WINDOW_FOLLOW_MS);
/** Errors a read of a window that is gone, or being torn down, comes back with. */
const GONE_CODES = new Set(["WINDOW_NOT_FOUND", "ACTION_NOT_SUPPORTED"]);

/**
 * After an action, a pinned window that can no longer be read may have been
 * replaced by the app. Polls the app's windows until windowSuccessor accounts
 * for the pinned one or FOLLOW_MS passes. The run follows a single replacement,
 * drops its region (a ref root in the old window means nothing in the new one),
 * and otherwise stops rather than guess.
 */
const awaitSuccessor = async (app, pinned, before) => {
  const deadline = Date.now() + FOLLOW_MS;
  for (;;) {
    const seen = windowSuccessor(before, listWindows(app), pinned);
    if (["present", "replaced", "ambiguous"].includes(seen.state) || Date.now() >= deadline) return seen;
    await sleep(250);
  }
};

export const run = async function* (
  goal,
  app,
  { root = null, windowId = null, text = null, cursor = false, values = true, confirmDestructive = false } = {},
) {
  if (!process.env.TYPESAFE_API_KEY) throw new Error("TYPESAFE_API_KEY unset");
  const supply = textSupply(text);
  const session = cursor ? startCursor(goal) : null;
  const clipboard = clipboardGuard();
  const knownInputs = typeof text === "function" ? [] : Array.isArray(text) ? text : [text];
  const state = {
    steps: 0, calls: 0, history: [], operation: null, root,
    suppliedInputs: new Set(knownInputs.filter((value) => typeof value === "string" && value.trim())),
  };
  let pinned = windowId;
  try {
    let last = null;
    for (;;) {
      let seen;
      try {
        seen = observe(app, state.root, pinned);
      } catch (error) {
        let stop = windowStop(error, pinned);
        if (!stop && pinned && error.code === "ACTION_NOT_SUPPORTED"
          && windowSuccessor(null, listWindows(app), pinned).state !== "present") stop = "window_closed";
        if (!stop) throw error;
        yield { stop, screen: last, history: state.history };
        return;
      }
      const { nodes, screen } = seen;
      last = screen;
      const before = fingerprint(nodes);
      const space = actionSpace(nodes, { drillable: !state.root, typable: supply.available(), values });
      if (!space.elements.length) {
        yield { stop: "nothing on this screen can be acted on", screen };
        return;
      }
      const answers = (await decide(goal, screen, space, state.history, values)).answers;
      state.calls += 1;
      const options = [
        ...Object.keys(space.targets),
        ...(state.root ? ["WIDEN"] : []),
        ...Object.keys(TERMINALS),
      ];
      state.operation = validateChoice(answers.operation, options).choice;
      const stop = shouldStop(state);
      if (stop) {
        yield {
          stop,
          screen,
          why: answers.operation.probabilities,
          confidence: answers.operation.confidence,
          history: state.history,
        };
        return;
      }
      let node = null;
      let confidence = answers.operation.confidence;
      if (space.targets[state.operation]) {
        const head = validateChoice(
          answers[`${state.operation.toLowerCase()}_target`],
          Object.keys(space.targets[state.operation]),
        );
        node = space.targets[state.operation][head.choice];
        confidence = head.confidence;
        let destructive = null;
        if (needsRiskCheck(confidence, { confirmDestructive })) {
          const rated = await rateRisk(goal, screen, state.operation, node, values);
          state.calls += 1;
          const answer = rated.answers?.destructive?.noul;
          destructive = typeof answer === "number" ? answer : 1;
        }
        const settled = route({
          target: head.choice,
          targetConfidence: confidence,
          present: null,
          destructive,
        }, { confirmDestructive });
        if (settled.decision !== "act") {
          yield {
            stop: `${settled.decision}: ${settled.why}`,
            screen,
            candidate: { operation: state.operation, target: criterion(node, head.choice), confidence },
            history: state.history,
          };
          return;
        }
      }
      let value = null;
      if (state.operation === "TYPE_TEXT") {
        value = await supply.take(describe(node, true), state.history);
        if (typeof value === "string" && value.trim()) {
          state.suppliedInputs.add(value);
          state.history = state.history.map((turn) => redactInputs(turn, state.suppliedInputs));
        }
        if (typeof value !== "string" || !value.trim()) {
          yield { stop: `no value was supplied for ${criterion(node, "?")}`, screen, history: state.history };
          return;
        }
      }
      const windowsBefore = pinned ? listWindows(app) : null;
      const outcome = await execute({ app, windowId: pinned }, state.operation, node, value, clipboard);
      state.steps += 1;
      let turn = {
        step: state.steps,
        operation: state.operation,
        target: node ? `${node.role}${node.name ? ` "${node.name}"` : ""}` : null,
        ref: node?.ref_id ?? null,
        confidence,
        ok: outcome.ok,
        delivery: outcome.delivery,
        steps: outcome.steps ?? [],
        post_state: outcome.post_state ?? null,
        details: outcome.details ?? null,
        error: outcome.error ?? null,
        route: outcome.route ?? null,
        truncated: space.truncated,
        changed: null,
      };
      turn = redactInputs(turn, state.suppliedInputs);
      state.history.push(turn);
      if (!outcome.ok) {
        yield { turn, screen };
        yield { stop: "action_failed", screen, error: redactInputs(outcome.error, state.suppliedInputs), history: state.history };
        return;
      }
      if ("root" in outcome) state.root = outcome.root;
      let after;
      try {
        after = "root" in outcome ? null : observe(app, state.root, pinned);
      } catch (error) {
        if (!pinned || !GONE_CODES.has(error.code)) {
          const stop = windowStop(error, pinned);
          if (!stop) throw error;
          turn.changed = true;
          yield { turn, screen };
          yield { stop, screen, history: state.history };
          return;
        }
        const next = await awaitSuccessor(app, pinned, windowsBefore);
        if (next.state === "present") throw error;
        turn.changed = true;
        if (next.state === "replaced") {
          const change = redactInputs({ from: pinned, to: next.id, title: next.title }, state.suppliedInputs);
          turn.window_changed = { from: change.from, to: change.to };
          yield { turn, screen };
          yield { window_changed: change };
          pinned = next.id;
          state.root = null;
          const settled = shouldStop(state);
          if (settled) {
            yield { stop: settled, screen, history: state.history };
            return;
          }
          continue;
        }
        yield { turn, screen };
        yield {
          stop: next.state === "ambiguous" ? "window_ambiguous" : "window_closed",
          screen,
          ...(next.candidates ? { candidates: next.candidates } : {}),
          history: state.history,
        };
        return;
      }
      turn.changed = "root" in outcome || fingerprint(after.nodes) !== before;
      yield { turn, screen };
      const settled = shouldStop(state);
      if (settled) {
        yield { stop: settled, screen, history: state.history };
        return;
      }
    }
  } finally {
    clipboard.restore();
    if (session) stopCursor();
  }
};

const main = async (argv) => {
  const flag = (name) => {
    const at = argv.indexOf(`--${name}`);
    if (at === -1) return null;
    const value = argv[at + 1];
    if (value === undefined || value.startsWith("--")) {
      console.error(`--${name} needs a value`);
      process.exit(2);
    }
    return value;
  };
  const app = flag("app");
  const root = flag("root");
  const windowId = flag("window-id");
  const text = argv.flatMap((a, i) => (argv[i - 1] === "--text" ? [a] : []));
  const cursor = argv.includes("--cursor");
  const values = !argv.includes("--no-values");
  const confirmDestructive = argv.includes("--confirm-destructive");
  const goal = argv
    .filter((a, i) => !a.startsWith("--") && !(argv[i - 1]?.startsWith("--") && argv[i - 1] !== "--cursor" && argv[i - 1] !== "--no-values" && argv[i - 1] !== "--confirm-destructive"))
    .join(" ");
  if (!app || !goal) {
    console.error(
      'usage: run.mjs --app <name> [--window-id <id>] [--cursor] [--no-values] [--confirm-destructive] [--root @ref] [--text "value"]... "<goal>"',
    );
    process.exit(2);
  }
  for await (const event of run(goal, app, { root, windowId, text, cursor, values, confirmDestructive })) {
    console.log(JSON.stringify(event));
    if (event.stop && event.stop !== "done") process.exitCode = 1;
  }
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2)).catch((error) => {
    console.log(JSON.stringify({ ok: false, error: String(error.message ?? error) }));
    process.exitCode = 1;
  });
}

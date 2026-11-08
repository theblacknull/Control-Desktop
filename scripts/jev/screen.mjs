/**
 * Reading a screen: which elements exist, which of them a command can reach,
 * and how each one is described. Shared by the single-step resolver and the
 * loop, so both judge the same screen the same way.
 */

const quoted = (s) => (s ? ` "${s}"` : "");

export const collect = (tree) => {
  const found = [];
  const walk = (node, path, parentRole) => {
    const self = `${node.role}${quoted(node.name)}`;
    if (node.ref_id) found.push({ ...node, path, parentRole });
    const next = node.children?.length ? [...path, self] : path;
    for (const child of node.children ?? []) walk(child, next, node.role);
  };
  walk(tree, [], null);
  return found;
};

/** A sheet, menu or alert owns input while it is up, and the window tree marks
 *  its elements offscreen. Reading the surface is the only way to act on it. */
export const outsideWindow = (data, windowId) => {
  const ids = [data?.window?.id, data?.surface?.window_id].filter((id) => id !== undefined && id !== null);
  return ids.length === 0 || ids.some((id) => id !== windowId);
};

export const overlayRole = (tree) => {
  let found = null;
  const walk = (n) => {
    if (!found && ["sheet", "alert", "menu", "popover"].includes(n.role)) found = n.role;
    for (const c of n.children ?? []) walk(c);
  };
  walk(tree);
  return found;
};

/**
 * Some modal panels never appear in the window's tree. The macOS open and save
 * panel that an Electron app such as Obsidian raises is one: the window snapshot
 * holds no trace of it, `--surface sheet` and `--surface alert` find nothing, yet
 * `list-surfaces` reports it and `--surface focused` reads it, rooted at a
 * `dialog`. While it is up, every element of the window behind it is the wrong
 * target. These checks decide when to look for such a panel and whether the
 * focused read found one rather than the window itself.
 */
export const DETACHED_SURFACE = "focused";
const DETACHED_ROLES = ["dialog", "sheet", "alert", "popover", "menu"];

/** `list-surfaces` data: true when the app reports any open surface at all. */
export const surfaceListed = (data) => (data?.surfaces ?? []).length > 0;

/** A `--surface focused` read: the overlay's role when it is rooted at one,
 *  null when it returned the ordinary window. */
export const detachedOverlay = (data) =>
  DETACHED_ROLES.includes(data?.tree?.role) ? data.tree.role : null;

/**
 * An action can make an app swap its window: Obsidian closes its start window
 * and opens the vault in a new one. A run pinned with --window-id then reads a
 * window that is gone (`WINDOW_NOT_FOUND`), or one being torn down
 * (`ACTION_NOT_SUPPORTED`). Given the app's windows before the action and now,
 * this says what happened to the pinned one:
 *   present    it is still there and readable;
 *   replaced   it is gone and exactly one new window appeared: follow it;
 *   ambiguous  it is gone and several new windows appeared: do not guess;
 *   closed     it is gone and nothing new appeared (yet);
 *   unknown    the window list could not be read.
 * Only windows absent before the action count as new, so an unrelated window
 * that was already open is never adopted.
 */
export const windowSuccessor = (before, after, pinned) => {
  if (!Array.isArray(after)) return { state: "unknown" };
  const live = (w) => w.visible !== false && !w.minimized && w.accessible !== false;
  if (after.some((w) => w.id === pinned && live(w))) return { state: "present" };
  const known = new Set((before ?? []).map((w) => w.id));
  const fresh = after.filter((w) => w.id !== pinned && !known.has(w.id) && live(w));
  if (fresh.length === 1) return { state: "replaced", id: fresh[0].id, title: fresh[0].title ?? null };
  if (fresh.length > 1) return { state: "ambiguous", candidates: fresh.map((w) => w.id) };
  return { state: "closed" };
};

/**
 * The only elements withheld are ones no command can reach. Everything else is
 * offered: the docs are explicit that a Choice does better with the full list
 * than a shortlist, and an unnamed row is still distinguishable by the value it
 * holds.
 *
 * An element that advertises no action is the one that must go. A sidebar
 * category in Numbers publishes its label on a cell and its behaviour on the
 * row around it, and both carry the same name, so the inert cell wins the
 * choice about half the time and every such win ends in POLICY_DENIED. Nothing
 * is lost by withholding it: the row beside it is the element that acts.
 */
export const offerable = (refs) =>
  refs.filter((n) => {
    const s = n.states ?? [];
    if (s.includes("disabled") || s.includes("hidden")) return false;
    if (!n.available_actions?.length) return false;
    return !(n.role === "cell" && n.parentRole === "treeitem");
  });

/**
 * Structured criteria disambiguate better than a flat sentence. Position is
 * spent only on an element with no name, no description and no value: a form
 * field in a PDF carries none of those, and where it sits is the only thing
 * that tells one from the next.
 */
export const describe = (node, rich, { values = true } = {}) => {
  const d = {
    what: `${node.role}${quoted(node.name ?? node.description)}`,
    where: node.path.length ? node.path.join(" > ") : "top level",
  };
  if (values && node.value != null && node.value !== "") d.holds = String(node.value).slice(0, 120);
  if (node.states?.length) d.state = node.states.join(", ");
  if (!d.what.includes('"') && node.bounds) {
    d.at = `x ${Math.round(node.bounds.x)}, y ${Math.round(node.bounds.y)}`;
  }
  if (rich) {
    if (node.available_actions?.length) d.supports = node.available_actions.join(", ");
    if (node.children_count) d.contains = `${node.children_count} items not shown`;
  }
  return d;
};

export const label = (n) => `${n.role}${quoted(n.name ?? n.description)}`;

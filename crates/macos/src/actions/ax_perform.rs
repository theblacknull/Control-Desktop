use agent_desktop_core::AdapterError;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use crate::actions::ax_mutation;
    use crate::tree::AXElement;
    use core_foundation::{base::TCFType, string::CFString};

    /// Decides delivery from what the application did, not what it claimed.
    /// Unadvertised actions are never performed, because a responder can answer
    /// `kAXErrorSuccess` to an action it never published and never ran. Only an
    /// uninformative code needs the settle poll to decide delivery at all; a
    /// reported success is already delivered, so observing it merely upgrades
    /// the report to verified and must not cost the caller a wait.
    pub(crate) fn perform_observed_action(
        el: &AXElement,
        name: &str,
        deadline: agent_desktop_core::Deadline,
    ) -> Result<crate::actions::chain::chain_delivery::DeliveryOutcome, AdapterError> {
        use crate::actions::activation_effect;
        use crate::actions::chain::chain_delivery::DeliveryOutcome;
        use ax_mutation::PerformSignal;

        if advertises_action(el, name, deadline) == Some(false) {
            return Ok(DeliveryOutcome::NotDelivered);
        }
        let before = activation_effect::focus_state(el, deadline);
        let action = CFString::new(name);
        let error = crate::tree::properties::ax_ipc::perform_action(
            el,
            action.as_concrete_TypeRef(),
            deadline,
        )?;
        let signal = match ax_mutation::classify_perform(name, error) {
            Ok(signal) => signal,
            Err(failure)
                if failure.disposition == agent_desktop_core::DeliverySemantics::uncertain() =>
            {
                return ax_mutation::settle_uninformative_perform(
                    name,
                    error,
                    activation_effect::settled_change(&before, el, deadline),
                )
                .map_err(|_| failure);
            }
            Err(failure) => return Err(failure),
        };
        match signal {
            PerformSignal::ReportedUnsupported => Ok(DeliveryOutcome::NotDelivered),
            PerformSignal::ReportedDelivered => Ok(DeliveryOutcome::from_delivery(
                true,
                activation_effect::changed_now(&before, el, deadline),
            )),
            PerformSignal::Uninformative => ax_mutation::settle_uninformative_perform(
                name,
                error,
                activation_effect::settled_change(&before, el, deadline),
            ),
        }
    }

    pub(crate) fn advertises_action(
        el: &AXElement,
        name: &str,
        deadline: agent_desktop_core::Deadline,
    ) -> Option<bool> {
        let mut usage = crate::tree::observation_usage::ObservationUsage::with_defaults();
        let read = crate::tree::properties::capabilities::copy_action_names_with_status(
            el,
            std::time::Instant::now() + deadline.remaining(),
            &mut usage,
        );
        let absent = crate::tree::properties::ax_absence::is_absent_action_error;
        read.value
            .map(|actions| actions.iter().any(|action| action == name))
            .or_else(|| read.error.filter(|error| absent(*error)).map(|_| false))
    }

    pub(crate) fn press_or_open(
        el: &AXElement,
        deadline: agent_desktop_core::Deadline,
    ) -> Result<bool, AdapterError> {
        let actions = action_names(el, deadline);
        for name in activation_plan(actions.as_deref()) {
            if perform_listed_action(el, name, deadline)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The actions worth attempting, from what the element advertises.
    ///
    /// A responder can answer `kAXErrorSuccess` to an action it never
    /// published and never ran, so nothing unadvertised is attempted once the
    /// list is known. When the list itself cannot be read the conventional
    /// order is tried once; the first code that proves the action never started
    /// still moves on to the next one.
    ///
    /// An item that publishes both activations keeps `AXOpen` behind `AXPress`:
    /// some AppKit rows list `AXPress` and still answer it with
    /// `kAXErrorActionUnsupported`, so the plan must be able to fall through to
    /// the menu-backed open instead of reporting the item unactivatable.
    pub(super) fn activation_plan(actions: Option<&[String]>) -> Vec<&'static str> {
        let Some(actions) = actions else {
            return vec!["AXPress", "AXOpen"];
        };
        let advertises = |name: &str| actions.iter().any(|it| it == name);
        ["AXPress", "AXOpen"]
            .into_iter()
            .filter(|name| advertises(name))
            .collect()
    }

    fn action_names(el: &AXElement, deadline: agent_desktop_core::Deadline) -> Option<Vec<String>> {
        let mut usage = crate::tree::observation_usage::ObservationUsage::with_defaults();
        crate::tree::properties::capabilities::copy_action_names_with_status(
            el,
            std::time::Instant::now() + deadline.remaining(),
            &mut usage,
        )
        .value
    }

    /// Performs one advertised action and reports whether it was delivered.
    ///
    /// `kAXErrorCannotComplete` and `kAXErrorFailure` can be returned after the
    /// action already started, so they end the sequence with an uncertain error
    /// instead of a retry through another action or a synthetic key. Only a
    /// clearly unsupported action returns `false` here.
    fn perform_listed_action(
        el: &AXElement,
        name: &str,
        deadline: agent_desktop_core::Deadline,
    ) -> Result<bool, AdapterError> {
        let action = CFString::new(name);
        let error = crate::tree::properties::ax_ipc::perform_action(
            el,
            action.as_concrete_TypeRef(),
            deadline,
        )?;
        ax_mutation::classify_result(el, name, ax_mutation::PERFORM_API, error)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;
    use crate::tree::AXElement;

    pub fn advertises_action(
        _el: &AXElement,
        _name: &str,
        _deadline: agent_desktop_core::Deadline,
    ) -> Option<bool> {
        None
    }

    pub fn perform_observed_action(
        _el: &AXElement,
        _name: &str,
        _deadline: agent_desktop_core::Deadline,
    ) -> Result<crate::actions::chain::chain_delivery::DeliveryOutcome, AdapterError> {
        Ok(crate::actions::chain::chain_delivery::DeliveryOutcome::NotDelivered)
    }

    pub fn press_or_open(
        _el: &AXElement,
        _deadline: agent_desktop_core::Deadline,
    ) -> Result<bool, AdapterError> {
        Ok(false)
    }
}

pub(crate) use imp::{advertises_action, perform_observed_action, press_or_open};

#[cfg(all(test, target_os = "macos"))]
#[path = "ax_perform_tests.rs"]
mod tests;

//! OpenShell boxes, one per tenant.
#[cfg(feature = "modules")]
mod bus_driver;
mod registry;
#[cfg(test)]
#[path = "scripted_driver_fixture.rs"]
pub(crate) mod scripted_driver;
#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

pub(crate) use crate::core_host::profiles::socket_ticket::{mint, verify, AUDIENCE};

#[cfg(test)]
#[path = "socket_ticket_tests.rs"]
mod tests;

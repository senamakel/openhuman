pub(crate) use crate::core_host::profiles::socket_ticket::{mint, secret, verify, Claims, AUDIENCE, SECRET_ENV, TTL_SECS};

#[cfg(test)]
#[path = "socket_ticket_tests.rs"]
mod tests;

use super::*;

static BROWSER_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[path = "browser_tests_computer_use_and_schema_tests.rs"]
mod computer_use_and_schema_tests;
#[path = "browser_tests_domain_and_parsing_tests.rs"]
mod domain_and_parsing_tests;

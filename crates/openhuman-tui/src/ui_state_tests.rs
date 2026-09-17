use super::*;

#[test]
fn ui_starts_on_chat_and_tabs_wrap_in_product_order() {
    let ui = UiState::new("thread".into(), "client".into());
    assert_eq!(ui.active_tab, AppTab::Chat);
    assert_eq!(AppTab::Logs.next(), AppTab::Chat);
    assert_eq!(AppTab::Chat.next(), AppTab::Config);
    assert_eq!(AppTab::Config.next(), AppTab::Settings);
    assert_eq!(AppTab::Settings.next(), AppTab::Logs);
    assert_eq!(AppTab::Logs.previous(), AppTab::Settings);
}

//! Shared Chrome DevTools Protocol client for the CEF-backed scanners.
//!
//! All CDP traffic flows through the in-process transport in
//! [`in_process`]: CDP messages travel directly between the Tauri shell
//! and the embedded CEF browser via `Webview::send_dev_tools_message`
//! and `Webview::on_dev_tools_protocol`. There is no listener and no
//! network surface; any same-UID process is shut out by construction.
//!
//! Scanners pick up a [`CdpConn`] either via [`target::conn_for_account`] (for
//! `acct_<id>`-labelled webviews) or [`target::conn_for_label`] /
//! [`target::connect_and_attach_matching_in_process_by_label`] (for other
//! surfaces such as the Meet call window).

#![allow(dead_code)] // Account-scanner helpers remain shared with the Meet CDP transport.

pub mod conn;
pub mod in_process;
pub mod target;

pub use conn::CdpConn;
pub use in_process::{install_for_label, set_cef_app_handle, CdpRegistry};

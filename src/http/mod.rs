//! HTTP server for ming-qiao
//!
//! This module implements the HTTP/REST API that Thales and the Merlin dashboard
//! use to interact with ming-qiao. It exposes endpoints for reading/sending messages,
//! querying decisions, and managing threads.
//!
//! Default port: 7777
//! Base URL: http://localhost:7777
//! WebSocket: ws://localhost:7777/ws
//! Merlin notifications: ws://localhost:7777/merlin/notifications

pub mod auth;
pub mod handlers;
pub mod merlin;
pub mod routes;
pub mod server;
pub mod ws;

pub use merlin::merlin_notifications_ws;
pub use server::HttpServer;
pub use ws::ws_handler;

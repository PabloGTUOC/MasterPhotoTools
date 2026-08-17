//! Google Photos client (F15)

pub mod album;
pub mod auth;
pub mod uploader;

pub use album::AlbumManager;
pub use auth::OAuth2Manager;
pub use uploader::Uploader;

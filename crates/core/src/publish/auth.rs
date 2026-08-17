use crate::error::Error;
use crate::ledger::Ledger;

pub struct OAuth2Manager<'a> {
    ledger: &'a Ledger,
}

impl<'a> OAuth2Manager<'a> {
    pub fn new(ledger: &'a Ledger) -> Self {
        Self { ledger }
    }

    pub fn get_bearer_token(&self) -> Result<String, Error> {
        // Mock token fetching
        Ok("mock_access_token".to_string())
    }

    pub fn save_token(
        &self,
        provider: &str,
        token: &str,
        scope: &str,
        expires_at: i64,
    ) -> Result<(), Error> {
        self.ledger
            .set_oauth_token(provider, token, scope, expires_at)
            .map_err(|e| Error::Internal(e.to_string()))
    }
}

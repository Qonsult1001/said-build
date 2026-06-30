//! Search API handler — thin binding over the service.

use sca_core::IdGenerator;

use crate::error::SearchResult;
use crate::ports::{LeadRepository, PreferencesRepository};
use crate::search::{
    CaptureLeadRequest, LeadDto, PreferencesDto, SearchRequest, SearchService,
    SetPreferencesRequest,
};

pub struct SearchApi<'a> {
    service: SearchService<'a>,
}

impl<'a> SearchApi<'a> {
    pub fn new(
        leads: &'a dyn LeadRepository,
        prefs: &'a dyn PreferencesRepository,
        ids: &'a dyn IdGenerator,
    ) -> Self {
        Self {
            service: SearchService::new(leads, prefs, ids),
        }
    }

    pub fn search(&self, req: SearchRequest) -> SearchResult<Vec<LeadDto>> {
        self.service.search(req)
    }

    pub fn capture_lead(&self, req: CaptureLeadRequest) -> SearchResult<LeadDto> {
        self.service.capture_lead(req)
    }

    pub fn set_preferences(&self, req: SetPreferencesRequest) -> SearchResult<PreferencesDto> {
        self.service.set_preferences(req)
    }
}

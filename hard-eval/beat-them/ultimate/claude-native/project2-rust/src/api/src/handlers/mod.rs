//! API handlers — thin presenters over the four capability services.
//!
//! Each `fn` returns `anyhow::Result<Dto>`: it builds the `AuthContext`, calls one
//! service method, and propagates errors. No validation, authorization, querying,
//! or mapping lives here — all of that is in the capability services. This file is
//! the "thin surface over core" pattern.

use anyhow::Result;
use core::{AuthContext, Token, UserId};

use accounts::dto::{SessionDto, UserProfileDto};
use accounts::login::LoginService;
use accounts::profile::ProfileService;
use accounts::Credentials;

use coverage::dto::CoverageDto;
use coverage::list_coverages::CoverageService;

use products::catalog::ProductService;
use products::dto::{FeatureDataDto, ProductDto};
use products::feature_data::FeatureDataService;

use search::dto::{LeadDto, PreferencesDto, SearchHitDto};
use search::leads::LeadService;
use search::preferences::PreferencesService;
use search::query::SearchService;

/// Composition root: every capability service wired once, shared by all handlers.
pub struct AppState {
    pub login: LoginService,
    pub profile: ProfileService,
    pub coverage: CoverageService,
    pub search: SearchService,
    pub leads: LeadService,
    pub preferences: PreferencesService,
    pub products: ProductService,
    pub feature_data: FeatureDataService,
}

impl AppState {
    /// Build the application by calling each capability's `build_default` factory.
    /// The surface never names a concrete adapter — only the factories.
    pub fn build() -> Self {
        let (login, profile) = accounts::build_default();
        let coverage = coverage::build_default();
        let (search, leads, preferences) = search::build_default();
        let (products, feature_data) = products::build_default();
        Self {
            login,
            profile,
            coverage,
            search,
            leads,
            preferences,
            products,
            feature_data,
        }
    }
}

// ── Accounts ────────────────────────────────────────────────────────────────

pub fn login(state: &AppState, username: String, password: String) -> Result<SessionDto> {
    let dto = state.login.login(Credentials { username, password })?;
    Ok(dto)
}

pub fn user_profile(state: &AppState, token: &str) -> Result<UserProfileDto> {
    let dto = state.profile.user_profile(&Token::new(token))?;
    Ok(dto)
}

// ── Coverage ──────────────────────────────────────────────────────────────────

pub fn user_coverages(state: &AppState, user_id: UserId) -> Result<Vec<CoverageDto>> {
    let ctx = AuthContext::authenticated(user_id);
    let dto = state.coverage.user_coverages(&ctx)?;
    Ok(dto)
}

// ── Search ──────────────────────────────────────────────────────────────────

pub fn search(state: &AppState, query: &str) -> Result<Vec<SearchHitDto>> {
    let dto = state.search.search(query)?;
    Ok(dto)
}

pub fn capture_lead(
    state: &AppState,
    user_id: UserId,
    email: String,
    note: String,
) -> Result<LeadDto> {
    let ctx = AuthContext::authenticated(user_id);
    let dto = state.leads.capture(&ctx, email, note)?;
    Ok(dto)
}

pub fn user_preferences(state: &AppState, user_id: UserId) -> Result<PreferencesDto> {
    let ctx = AuthContext::authenticated(user_id);
    let dto = state.preferences.user_preferences(&ctx)?;
    Ok(dto)
}

// ── Products ──────────────────────────────────────────────────────────────────

pub fn product(state: &AppState, sku: &str) -> Result<ProductDto> {
    let dto = state.products.product(sku)?;
    Ok(dto)
}

pub fn feature_data(state: &AppState, sku: &str) -> Result<FeatureDataDto> {
    let dto = state.feature_data.feature_data(sku)?;
    Ok(dto)
}

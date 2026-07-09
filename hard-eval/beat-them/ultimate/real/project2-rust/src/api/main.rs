//! `api` surface crate (Ring 3) — the one outer interface.
//!
//! Wires the four capability crates over the kernel's in-memory adapters and
//! exposes them as a single application. Holds NO business logic: every handler
//! is a one-liner over a capability's `*Api`. This is the thin-surface-over-core
//! pattern — the surface depends on the capabilities and `core`, never on
//! another surface, and never reaches into a capability's internals.
//!
//! `anyhow` is permitted *here* (a binary), forbidden in the libraries above.

use anyhow::Result;

use sca_core::{
    AccountId, CoverageId, InMemoryRepository, LeadId, ProductId, SequentialIdGenerator,
};

use accounts::{
    Account, AccountsApi, DemoPasswordHasher, LoginRequest, RegisterRequest,
};
use coverage::{Coverage, CoverageApi, CreateCoverageRequest};
use products::{CreateProductRequest, Product, ProductsApi};
use search::{
    CaptureLeadRequest, Lead, SearchApi, SearchRequest, SetPreferencesRequest,
    UserPreferences,
};

/// Owns the wired stores and exposes each capability's API. Built once and held
/// for the process lifetime (in a real deploy, behind an HTTP router).
struct App {
    account_repo: InMemoryRepository<AccountId, Account>,
    coverage_repo: InMemoryRepository<CoverageId, Coverage>,
    lead_repo: InMemoryRepository<LeadId, Lead>,
    pref_repo: InMemoryRepository<AccountId, UserPreferences>,
    product_repo: InMemoryRepository<ProductId, Product>,
    hasher: DemoPasswordHasher,
    account_ids: SequentialIdGenerator,
    coverage_ids: SequentialIdGenerator,
    search_ids: SequentialIdGenerator,
    product_ids: SequentialIdGenerator,
}

impl App {
    fn new() -> Self {
        Self {
            account_repo: InMemoryRepository::new(),
            coverage_repo: InMemoryRepository::new(),
            lead_repo: InMemoryRepository::new(),
            pref_repo: InMemoryRepository::new(),
            product_repo: InMemoryRepository::new(),
            hasher: DemoPasswordHasher,
            account_ids: SequentialIdGenerator::new("acct"),
            coverage_ids: SequentialIdGenerator::new("cov"),
            search_ids: SequentialIdGenerator::new("lead"),
            product_ids: SequentialIdGenerator::new("prod"),
        }
    }

    fn accounts(&self) -> AccountsApi<'_> {
        AccountsApi::new(&self.account_repo, &self.hasher, &self.account_ids)
    }
    fn coverages(&self) -> CoverageApi<'_> {
        CoverageApi::new(&self.coverage_repo, &self.coverage_ids)
    }
    fn search(&self) -> SearchApi<'_> {
        SearchApi::new(&self.lead_repo, &self.pref_repo, &self.search_ids)
    }
    fn products(&self) -> ProductsApi<'_> {
        ProductsApi::new(&self.product_repo, &self.product_ids)
    }
}

fn main() -> Result<()> {
    let app = App::new();

    // Accounts: register -> login -> profile
    let token = app.accounts().register(RegisterRequest {
        email: "ada@example.com".into(),
        password: "supersecret".into(),
        display_name: "Ada Lovelace".into(),
    })?;
    let session = app.accounts().login(LoginRequest {
        email: "ada@example.com".into(),
        password: "supersecret".into(),
    })?;
    let profile = app.accounts().userprofile(&session.account_id)?;
    println!("account {} -> {}", profile.account_id, profile.display_name);

    // Coverage: open a global coverage for the user
    let cov = app.coverages().create(CreateCoverageRequest {
        owner_id: token.account_id.clone(),
        region: "EU".into(),
        limit_cents: 50_000,
    })?;
    let mine = app.coverages().list_for_user(&token.account_id)?;
    println!("coverage {} ({} of {} total)", cov.region, mine.len(), mine.len());

    // Search: set prefs, capture leads, query
    app.search().set_preferences(SetPreferencesRequest {
        owner_id: token.account_id.clone(),
        page_size: 5,
        safe_search: true,
    })?;
    app.search().capture_lead(CaptureLeadRequest {
        owner_id: token.account_id.clone(),
        title: "Rust backend role".into(),
        keywords: vec!["rust".into(), "backend".into()],
        score: 92,
    })?;
    let results = app.search().search(SearchRequest {
        owner_id: token.account_id.clone(),
        query: "rust".into(),
    })?;
    println!("search 'rust' -> {} hit(s)", results.len());

    // Products: create -> featuredata read -> featured listing
    let p = app.products().create(CreateProductRequest {
        name: "Pro Plan".into(),
        price_cents: 4900,
        headline: "Everything unlocked".into(),
        bullets: vec!["Unlimited searches".into()],
        featured: true,
    })?;
    let fd = app.products().featuredata(&p.id)?;
    let featured = app.products().product(true)?;
    println!("product {} -> '{}' ({} featured)", p.name, fd.headline, featured.len());

    Ok(())
}

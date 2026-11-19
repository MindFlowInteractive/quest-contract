#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, String,
    Symbol, Vec,
};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Treasury,
    Token,
    Panel,
    NextReportId,
    Report(u64),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReportStatus {
    Open,
    Upheld,
    Rejected,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    pub id: u64,
    pub reporter: Address,
    pub target: Address,
    pub reason: String,
    pub bond: i128,
    pub status: ReportStatus,
    pub resolver: Option<Address>,
    pub created_at: u64,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum DisputeBondError {
    AlreadyInitialized = 1,
    InvalidAmount = 2,
    ReportNotFound = 3,
    AlreadyResolved = 4,
    Unauthorized = 5,
    InvalidStatus = 6,
}

const REPORT_FILED: Symbol = symbol_short!("rep_filed");
const REPORT_RESOLVED: Symbol = symbol_short!("rep_reslv");
const PANEL_SET: Symbol = symbol_short!("panel_set");

#[contract]
pub struct DisputeBondContract;

#[contractimpl]
impl DisputeBondContract {
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        treasury: Address,
    ) -> Result<(), DisputeBondError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(DisputeBondError::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::NextReportId, &1u64);
        env.storage().instance().set(&DataKey::Panel, &Vec::new(&env));
        Ok(())
    }

    /// Lock a bond and open a report against a puzzle or player.
    pub fn file_report(
        env: Env,
        reporter: Address,
        target: Address,
        reason: String,
        bond: i128,
    ) -> Result<u64, DisputeBondError> {
        reporter.require_auth();
        if bond <= 0 {
            return Err(DisputeBondError::InvalidAmount);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextReportId)
            .unwrap_or(1);

        let report = Report {
            id,
            reporter: reporter.clone(),
            target,
            reason,
            bond,
            status: ReportStatus::Open,
            resolver: None,
            created_at: env.ledger().timestamp(),
        };

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_address).transfer(
            &reporter,
            &env.current_contract_address(),
            &bond,
        );

        env.storage()
            .persistent()
            .set(&DataKey::Report(id), &report);
        env.storage()
            .instance()
            .set(&DataKey::NextReportId, &(id + 1));
        env.events().publish((REPORT_FILED, id), reporter);
        Ok(id)
    }

    /// Resolve an open report. Only the admin or a panel member may resolve.
    pub fn resolve_report(
        env: Env,
        resolver: Address,
        report_id: u64,
        outcome: ReportStatus,
    ) -> Result<(), DisputeBondError> {
        resolver.require_auth();

        if outcome == ReportStatus::Open {
            return Err(DisputeBondError::InvalidStatus);
        }

        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        let panel: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Panel)
            .unwrap_or_else(|| Vec::new(&env));
        if resolver != admin && !panel.contains(&resolver) {
            return Err(DisputeBondError::Unauthorized);
        }

        let key = DataKey::Report(report_id);
        let mut report: Report = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(DisputeBondError::ReportNotFound)?;

        if report.status != ReportStatus::Open {
            return Err(DisputeBondError::AlreadyResolved);
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);

        match outcome {
            ReportStatus::Upheld => {
                token_client.transfer(
                    &env.current_contract_address(),
                    &report.reporter,
                    &report.bond,
                );
            }
            ReportStatus::Rejected => {
                let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
                token_client.transfer(
                    &env.current_contract_address(),
                    &treasury,
                    &report.bond,
                );
            }
            ReportStatus::Open => return Err(DisputeBondError::InvalidStatus),
        }

        report.status = outcome.clone();
        report.resolver = Some(resolver.clone());
        env.storage().persistent().set(&key, &report);
        env.events()
            .publish((REPORT_RESOLVED, report_id), (resolver, outcome));
        Ok(())
    }

    /// Replace the moderation panel (admin only).
    pub fn set_panel(env: Env, admin: Address, panel: Vec<Address>) -> Result<(), DisputeBondError> {
        admin.require_auth();
        let current_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if admin != current_admin {
            return Err(DisputeBondError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Panel, &panel);
        env.events().publish((PANEL_SET,), panel);
        Ok(())
    }

    pub fn get_report(env: Env, report_id: u64) -> Option<Report> {
        env.storage().persistent().get(&DataKey::Report(report_id))
    }

    pub fn is_resolved(env: Env, report_id: u64) -> bool {
        let report: Option<Report> = env
            .storage()
            .persistent()
            .get(&DataKey::Report(report_id));
        match report {
            Some(report) => report.status != ReportStatus::Open,
            None => false,
        }
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
    }

    pub fn get_treasury(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Treasury).unwrap()
    }

    pub fn get_panel(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Panel)
            .unwrap_or_else(|| Vec::new(&env))
    }
}

#[cfg(test)]
mod test;

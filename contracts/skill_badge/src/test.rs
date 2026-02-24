#![no_std]

#[cfg(test)]
mod test {
    use super::super::src::*;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    fn setup(env: &Env) -> (SkillBadgeContractClient<'_>, Address) {
        env.mock_all_auths();
        let admin = Address::generate(env);
        let id = env.register_contract(None, SkillBadgeContract);
        let client = SkillBadgeContractClient::new(env, &id);
        client.initialize(&admin);
        (client, admin)
    }

    #[test]
    fn test_issuance_and_view() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let player = Address::generate(&env);

        client.issue_badge(&admin, &player, &BadgeCategory::Logic);
        
        let badge = client.get_badge(&player, &BadgeCategory::Logic).unwrap();
        assert_eq!(badge.level, BadgeLevel::Novice);
        assert_eq!(badge.category, BadgeCategory::Logic);
    }

    #[test]
    fn test_progression() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let player = Address::generate(&env);

        client.issue_badge(&admin, &player, &BadgeCategory::Math);
        
        // Upgrade to Apprentice
        let level1 = client.upgrade_badge(&admin, &player, &BadgeCategory::Math, &100);
        assert_eq!(level1, BadgeLevel::Apprentice);
        
        let badge = client.get_badge(&player, &BadgeCategory::Math).unwrap();
        assert_eq!(badge.level, BadgeLevel::Apprentice);
        assert_eq!(badge.verifier_score, 100);

        // Upgrade to Journeyman
        let level2 = client.upgrade_badge(&admin, &player, &BadgeCategory::Math, &200);
        assert_eq!(level2, BadgeLevel::Journeyman);
    }

    #[test]
    #[should_panic(expected = "HostError: Error(Contract, #4)")] // BadgeAlreadyExists
    fn test_duplicate_issuance() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let player = Address::generate(&env);

        client.issue_badge(&admin, &player, &BadgeCategory::Speed);
        client.issue_badge(&admin, &player, &BadgeCategory::Speed);
    }

    #[test]
    fn test_revocation() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let player = Address::generate(&env);

        client.issue_badge(&admin, &player, &BadgeCategory::Cryptography);
        assert!(client.get_badge(&player, &BadgeCategory::Cryptography).is_some());

        client.revoke_badge(&admin, &player, &BadgeCategory::Cryptography);
        assert!(client.get_badge(&player, &BadgeCategory::Cryptography).is_none());
    }

    #[test]
    fn test_showcase() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let player = Address::generate(&env);

        client.issue_badge(&admin, &player, &BadgeCategory::Logic);
        client.issue_badge(&admin, &player, &BadgeCategory::Math);
        
        client.add_to_showcase(&player, &BadgeCategory::Logic);
        
        let showcase = client.get_showcase(&player);
        assert_eq!(showcase.len(), 1);
        assert_eq!(showcase.get(0).unwrap(), BadgeCategory::Logic);
    }

    #[test]
    fn test_synergies() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let player = Address::generate(&env);

        client.issue_badge(&admin, &player, &BadgeCategory::Logic);
        client.issue_badge(&admin, &player, &BadgeCategory::Math);
        client.issue_badge(&admin, &player, &BadgeCategory::Cryptography);
        
        // 3 badges at level 0 (Novice). total levels = 3 * (0+1) = 3 + 5 bonus = 8
        let score = client.get_synergies(&player);
        assert_eq!(score, 8);
    }

    #[test]
    fn test_leaderboard() {
        let env = Env::default();
        let (client, admin) = setup(&env);
        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);

        client.issue_badge(&admin, &p1, &BadgeCategory::Logic);
        client.issue_badge(&admin, &p2, &BadgeCategory::Logic);
        client.upgrade_badge(&admin, &p1, &BadgeCategory::Logic, &100);

        let board = client.get_leaderboard(&BadgeCategory::Logic);
        assert_eq!(board.len(), 2);
    }
}

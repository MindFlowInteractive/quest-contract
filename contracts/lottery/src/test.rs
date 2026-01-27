#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        Env, Address, testutils::{Address as _, Ledger}
    };

    fn setup() -> (Env, Address) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        (env, admin)
    }
}

#[test]
fn test_ticket_purchase() {
    let (env, admin) = setup();
    let user = Address::generate(&env);

    Lottery::init(&env, admin.clone());
    Lottery::create_round(&env, 1, 10);

    Lottery::buy_ticket(&env, 1, user.clone());

    let round = Lottery::get_round(&env, 1);
    assert_eq!(round.tickets.len(), 1);
}

#[test]
#[should_panic]
fn test_no_ticket_after_close() {
    let (env, admin) = setup();
    let user = Address::generate(&env);

    Lottery::init(&env, admin.clone());
    Lottery::create_round(&env, 1, 10);
    Lottery::close_round(&env, 1);

    Lottery::buy_ticket(&env, 1, user);
}

#[test]
fn test_draw_winner() {
    let (env, admin) = setup();
    let user1 = Address::generate(&env);
    let user2 = Address::generate(&env);

    Lottery::init(&env, admin.clone());
    Lottery::create_round(&env, 1, 10);

    Lottery::buy_ticket(&env, 1, user1.clone());
    Lottery::buy_ticket(&env, 1, user2.clone());

    Lottery::close_round(&env, 1);
    Lottery::draw_winner(&env, 1);

    let round = Lottery::get_round(&env, 1);
    assert!(round.winner.is_some());
}


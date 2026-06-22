use odra::prelude::*;
use odra::casper_types::U256;

const STATUS_ACTIVE: u8 = 0;
const STATUS_SETTLED: u8 = 1;
const STATUS_EXPIRED: u8 = 2;

#[odra::odra_error]
pub enum Error {
    NotOwner = 1,
    ZeroAmount = 2,
    InsufficientBalance = 3,
    InsufficientLiquidity = 4,
    NotAgent = 5,
    PolicyNotActive = 6,
    NothingToClaim = 7,
}

#[odra::odra_type]
pub struct Policy {
    pub buyer: Address,
    pub premium: U256,
    pub payout: U256,
    pub threshold: u64,
    pub status: u8,
}

#[odra::event]
pub struct Deposited {
    pub provider: Address,
    pub amount: U256,
}

#[odra::event]
pub struct Withdrawn {
    pub provider: Address,
    pub amount: U256,
}

#[odra::event]
pub struct AgentUpdated {
    pub agent: Address,
}

#[odra::event]
pub struct PolicyIssued {
    pub id: u64,
    pub buyer: Address,
    pub premium: U256,
    pub payout: U256,
    pub threshold: u64,
}

#[odra::event]
pub struct PolicySettled {
    pub id: u64,
    pub buyer: Address,
    pub payout: U256,
}

#[odra::event]
pub struct PolicyExpired {
    pub id: u64,
}

#[odra::event]
pub struct Claimed {
    pub buyer: Address,
    pub amount: U256,
}

#[odra::module(
    events = [Deposited, Withdrawn, AgentUpdated, PolicyIssued, PolicySettled, PolicyExpired, Claimed],
    errors = Error
)]
pub struct InsuranceVault {
    owner: Var<Address>,
    agent: Var<Address>,
    total_liquidity: Var<U256>,
    locked: Var<U256>,
    lp_balance: Mapping<Address, U256>,
    claimable: Mapping<Address, U256>,
    policies: Mapping<u64, Policy>,
    next_policy_id: Var<u64>,
}

#[odra::module]
impl InsuranceVault {
    pub fn init(&mut self, agent: Address) {
        self.owner.set(self.env().caller());
        self.agent.set(agent);
        self.total_liquidity.set(U256::zero());
        self.locked.set(U256::zero());
    }

    pub fn deposit(&mut self, amount: U256) {
        if amount.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        let provider = self.env().caller();
        let current = self.lp_balance.get_or_default(&provider);
        self.lp_balance.set(&provider, current + amount);
        self.total_liquidity.set(self.total_liquidity.get_or_default() + amount);
        self.env().emit_event(Deposited { provider, amount });
    }

    pub fn withdraw(&mut self, amount: U256) {
        if amount.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        let provider = self.env().caller();
        let balance = self.lp_balance.get_or_default(&provider);
        if amount > balance {
            self.env().revert(Error::InsufficientBalance);
        }
        if amount > self.get_available_liquidity() {
            self.env().revert(Error::InsufficientLiquidity);
        }
        self.lp_balance.set(&provider, balance - amount);
        self.total_liquidity.set(self.total_liquidity.get_or_default() - amount);
        self.env().emit_event(Withdrawn { provider, amount });
    }

    pub fn issue_policy(
        &mut self,
        buyer: Address,
        premium: U256,
        payout: U256,
        threshold: u64,
    ) -> u64 {
        self.assert_agent();
        if payout.is_zero() {
            self.env().revert(Error::ZeroAmount);
        }
        if payout > self.get_available_liquidity() {
            self.env().revert(Error::InsufficientLiquidity);
        }

        let id = self.next_policy_id.get_or_default();
        let policy = Policy {
            buyer,
            premium,
            payout,
            threshold,
            status: STATUS_ACTIVE,
        };
        self.policies.set(&id, policy);
        self.next_policy_id.set(id + 1);

        self.total_liquidity.set(self.total_liquidity.get_or_default() + premium);
        self.locked.set(self.locked.get_or_default() + payout);

        self.env().emit_event(PolicyIssued {
            id,
            buyer,
            premium,
            payout,
            threshold,
        });
        id
    }

    pub fn settle_policy(&mut self, id: u64) {
        self.assert_agent();
        let mut policy = self.policies.get(&id).unwrap_or_revert(&self.env());
        if policy.status != STATUS_ACTIVE {
            self.env().revert(Error::PolicyNotActive);
        }
        let payout = policy.payout;
        let buyer = policy.buyer;
        policy.status = STATUS_SETTLED;
        self.policies.set(&id, policy);

        self.locked.set(self.locked.get_or_default() - payout);
        self.total_liquidity.set(self.total_liquidity.get_or_default() - payout);
        let owed = self.claimable.get_or_default(&buyer) + payout;
        self.claimable.set(&buyer, owed);

        self.env().emit_event(PolicySettled { id, buyer, payout });
    }

    pub fn expire_policy(&mut self, id: u64) {
        self.assert_agent();
        let mut policy = self.policies.get(&id).unwrap_or_revert(&self.env());
        if policy.status != STATUS_ACTIVE {
            self.env().revert(Error::PolicyNotActive);
        }
        let payout = policy.payout;
        policy.status = STATUS_EXPIRED;
        self.policies.set(&id, policy);

        self.locked.set(self.locked.get_or_default() - payout);

        self.env().emit_event(PolicyExpired { id });
    }

    pub fn claim(&mut self) {
        let buyer = self.env().caller();
        let owed = self.claimable.get_or_default(&buyer);
        if owed.is_zero() {
            self.env().revert(Error::NothingToClaim);
        }
        self.claimable.set(&buyer, U256::zero());
        self.env().emit_event(Claimed { buyer, amount: owed });
    }

    pub fn set_agent(&mut self, new_agent: Address) {
        self.assert_owner();
        self.agent.set(new_agent);
        self.env().emit_event(AgentUpdated { agent: new_agent });
    }

    pub fn get_owner(&self) -> Address {
        self.owner.get().unwrap_or_revert(&self.env())
    }

    pub fn get_agent(&self) -> Address {
        self.agent.get().unwrap_or_revert(&self.env())
    }

    pub fn get_total_liquidity(&self) -> U256 {
        self.total_liquidity.get_or_default()
    }

    pub fn get_available_liquidity(&self) -> U256 {
        self.total_liquidity.get_or_default() - self.locked.get_or_default()
    }

    pub fn lp_balance_of(&self, provider: Address) -> U256 {
        self.lp_balance.get_or_default(&provider)
    }

    pub fn claimable_of(&self, buyer: Address) -> U256 {
        self.claimable.get_or_default(&buyer)
    }

    pub fn get_policy(&self, id: u64) -> Policy {
        self.policies.get(&id).unwrap_or_revert(&self.env())
    }

    pub fn get_policy_count(&self) -> u64 {
        self.next_policy_id.get_or_default()
    }

    fn assert_owner(&self) {
        if self.env().caller() != self.owner.get().unwrap_or_revert(&self.env()) {
            self.env().revert(Error::NotOwner);
        }
    }

    fn assert_agent(&self) {
        if self.env().caller() != self.agent.get().unwrap_or_revert(&self.env()) {
            self.env().revert(Error::NotAgent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::Deployer;

    fn deploy() -> (odra::host::HostEnv, InsuranceVaultHostRef, Address, Address, Address) {
        let env = odra_test::env();
        let owner = env.get_account(0);
        let agent = env.get_account(1);
        let lp = env.get_account(2);
        let buyer = env.get_account(3);
        env.set_caller(owner);
        let vault = InsuranceVault::deploy(&env, InsuranceVaultInitArgs { agent });
        (env, vault, agent, lp, buyer)
    }

    #[test]
    fn deposit_and_withdraw() {
        let (env, mut vault, _agent, lp, _buyer) = deploy();
        env.set_caller(lp);
        vault.deposit(U256::from(1000));
        vault.withdraw(U256::from(400));
        assert_eq!(vault.lp_balance_of(lp), U256::from(600));
        assert_eq!(vault.get_total_liquidity(), U256::from(600));
    }

    #[test]
    fn issue_policy_locks_funds() {
        let (env, mut vault, agent, lp, buyer) = deploy();
        env.set_caller(lp);
        vault.deposit(U256::from(10_000));
        env.set_caller(agent);
        let id = vault.issue_policy(buyer, U256::from(100), U256::from(5_000), 20);
        assert_eq!(id, 0);
        assert_eq!(vault.get_total_liquidity(), U256::from(10_100));
        assert_eq!(vault.get_available_liquidity(), U256::from(5_100));
    }

    #[test]
    fn settle_pays_buyer() {
        let (env, mut vault, agent, lp, buyer) = deploy();
        env.set_caller(lp);
        vault.deposit(U256::from(10_000));
        env.set_caller(agent);
        vault.issue_policy(buyer, U256::from(100), U256::from(5_000), 20);
        vault.settle_policy(0);

        assert_eq!(vault.get_policy(0).status, STATUS_SETTLED);
        assert_eq!(vault.claimable_of(buyer), U256::from(5_000));
        assert_eq!(vault.get_total_liquidity(), U256::from(5_100));
        assert_eq!(vault.get_available_liquidity(), U256::from(5_100));

        env.set_caller(buyer);
        vault.claim();
        assert_eq!(vault.claimable_of(buyer), U256::zero());
    }

    #[test]
    fn expire_keeps_premium_and_releases_lock() {
        let (env, mut vault, agent, lp, buyer) = deploy();
        env.set_caller(lp);
        vault.deposit(U256::from(10_000));
        env.set_caller(agent);
        vault.issue_policy(buyer, U256::from(100), U256::from(5_000), 20);
        vault.expire_policy(0);

        assert_eq!(vault.get_policy(0).status, STATUS_EXPIRED);
        assert_eq!(vault.get_total_liquidity(), U256::from(10_100));
        assert_eq!(vault.get_available_liquidity(), U256::from(10_100));
    }

    #[test]
    fn cannot_settle_twice() {
        let (env, mut vault, agent, lp, buyer) = deploy();
        env.set_caller(lp);
        vault.deposit(U256::from(10_000));
        env.set_caller(agent);
        vault.issue_policy(buyer, U256::from(100), U256::from(5_000), 20);
        vault.settle_policy(0);
        let result = vault.try_settle_policy(0);
        assert!(result.is_err());
    }

    #[test]
    fn non_agent_cannot_settle() {
        let (env, mut vault, agent, lp, buyer) = deploy();
        env.set_caller(lp);
        vault.deposit(U256::from(10_000));
        env.set_caller(agent);
        vault.issue_policy(buyer, U256::from(100), U256::from(5_000), 20);
        env.set_caller(lp);
        let result = vault.try_settle_policy(0);
        assert!(result.is_err());
    }
}
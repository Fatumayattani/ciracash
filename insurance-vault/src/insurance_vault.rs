use odra::prelude::*;
use odra::casper_types::U256;

#[odra::odra_error]
pub enum Error {
    NotOwner = 1,
    ZeroAmount = 2,
    InsufficientBalance = 3,
    InsufficientLiquidity = 4,
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

#[odra::module(events = [Deposited, Withdrawn, AgentUpdated], errors = Error)]
pub struct InsuranceVault {
    owner: Var<Address>,
    agent: Var<Address>,
    total_liquidity: Var<U256>,
    locked: Var<U256>,
    lp_balance: Mapping<Address, U256>,
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

    fn assert_owner(&self) {
        if self.env().caller() != self.owner.get().unwrap_or_revert(&self.env()) {
            self.env().revert(Error::NotOwner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use odra::host::Deployer;

    #[test]
    fn deposit_and_withdraw() {
        let env = odra_test::env();
        let owner = env.get_account(0);
        let agent = env.get_account(1);
        let lp = env.get_account(2);

        env.set_caller(owner);
        let mut vault = InsuranceVault::deploy(&env, InsuranceVaultInitArgs { agent });

        assert_eq!(vault.get_agent(), agent);
        assert_eq!(vault.get_total_liquidity(), U256::zero());

        env.set_caller(lp);
        vault.deposit(U256::from(1000));
        assert_eq!(vault.get_total_liquidity(), U256::from(1000));
        assert_eq!(vault.lp_balance_of(lp), U256::from(1000));
        assert_eq!(vault.get_available_liquidity(), U256::from(1000));

        vault.withdraw(U256::from(400));
        assert_eq!(vault.lp_balance_of(lp), U256::from(600));
        assert_eq!(vault.get_total_liquidity(), U256::from(600));
    }
}
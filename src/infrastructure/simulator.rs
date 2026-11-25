use rand::{Rng, rngs::ThreadRng};

pub struct PnlSimulator {
    rng: ThreadRng,
    win_prob: f64,
}

impl PnlSimulator {
    pub fn new() -> Self {
        Self {
            rng: rand::rng(),
            win_prob: 0.55,
        }
    }

    pub fn next_trade(&mut self) -> f64 {
        if self.rng.random_bool(self.win_prob) {
            self.rng.random_range(0.2..2.5)
        } else {
            -self.rng.random_range(0.2..2.0)
        }
    }
}
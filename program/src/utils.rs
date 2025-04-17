use pubkey::Pubkey;

use crate::SEEDS;

pub fn derive_pda(base: Pubkey, space: u32, seed: u8) -> (Pubkey, u8) {
    let mut extra_seeds = space.to_le_bytes().to_vec();
    extra_seeds.push(seed);
    let seeds = &[base.as_ref(), SEEDS, &extra_seeds];
    Pubkey::find_program_address(seeds, &crate::ID)
}

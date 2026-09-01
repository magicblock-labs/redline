use pubkey::Pubkey;

pub(crate) fn pda_seed(space: u32, seed: u16, authority: Pubkey) -> Vec<u8> {
    let mut seeds = space.to_le_bytes().to_vec();
    let seed = seed.to_le_bytes();
    // Keep existing one-byte PDA addresses stable while extending the seed space.
    seeds.extend_from_slice(if seed[1] == 0 { &seed[..1] } else { &seed });
    seeds.extend_from_slice(&authority.as_ref()[..16]);
    seeds
}

pub fn derive_pda(base: Pubkey, space: u32, seed: u16, authority: Pubkey) -> (Pubkey, u8) {
    let seeds = pda_seed(space, seed, authority);
    let seeds = &[base.as_ref(), &seeds];
    Pubkey::find_program_address(seeds, &crate::ID)
}

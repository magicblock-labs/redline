use pubkey::Pubkey;

pub fn derive_pda(base: Pubkey, space: u32, seed: u8, authority: Pubkey) -> (Pubkey, u8) {
    let mut seeds = space.to_le_bytes().to_vec();
    seeds.push(seed);
    seeds.extend_from_slice(&authority.as_ref()[..16]);
    let seeds = &[base.as_ref(), &seeds];
    Pubkey::find_program_address(seeds, &crate::ID)
}

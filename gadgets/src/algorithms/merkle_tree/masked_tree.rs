use snarkos_errors::gadgets::SynthesisError;
use snarkos_models::{
    algorithms::CRH,
    curves::PrimeField,
    gadgets::{
        algorithms::MaskedCRHGadget,
        r1cs::ConstraintSystem,
        utilities::{uint8::UInt8, ToBytesGadget},
    },
};

/// Computes a root given `leaves`. Uses a nonce to mask the
/// computation, to ensure amortization resistance.
pub fn compute_root<H: CRH, HG: MaskedCRHGadget<H, F>, F: PrimeField, TB: ToBytesGadget<F>, CS: ConstraintSystem<F>>(
    mut cs: CS,
    parameters: &HG::ParametersGadget,
    mask: &TB,
    leaves: &[HG::OutputGadget],
) -> Result<HG::OutputGadget, SynthesisError> {
    // Mask is assumed to be derived from the nonce and the root, which will be checked by the
    // verifier.
    let mask_bytes = mask.to_bytes(cs.ns(|| "mask to bytes"))?;

    // Assume the leaves are already hashed.
    let mut current_leaves = leaves.to_vec();
    let mut level = 0;
    // Keep hashing pairs until there is only one element - the root.
    while current_leaves.len() != 1 {
        current_leaves = current_leaves
            .chunks(2)
            .enumerate()
            .map(|(i, left_right)| {
                let inner_hash = hash_inner_node_gadget::<H, HG, F, _, _>(
                    cs.ns(|| format!("hash left right {} on level {}", i, level)),
                    parameters,
                    &left_right[0],
                    &left_right[1],
                    &mask_bytes,
                );
                inner_hash
            })
            .collect::<Result<Vec<_>, _>>()?;
        level += 1;
    }

    let computed_root = current_leaves[0].clone();
    // Hash the root.

    Ok(computed_root)
}

pub(crate) fn hash_inner_node_gadget<H, HG, F, TB, CS>(
    mut cs: CS,
    parameters: &HG::ParametersGadget,
    left_child: &TB,
    right_child: &TB,
    mask: &[UInt8],
) -> Result<HG::OutputGadget, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
    H: CRH,
    HG: MaskedCRHGadget<H, F>,
    TB: ToBytesGadget<F>,
{
    let left_bytes = left_child.to_bytes(&mut cs.ns(|| "left_to_bytes"))?;
    let right_bytes = right_child.to_bytes(&mut cs.ns(|| "right_to_bytes"))?;
    let bytes = [left_bytes, right_bytes].concat();

    HG::check_evaluation_gadget_masked(cs, parameters, &bytes, &mask)
}

pub(crate) fn hash_leaf_gadget<H, HG, F, TB, CS>(
    mut cs: CS,
    parameters: &HG::ParametersGadget,
    leaf: &TB,
    mask: &[UInt8],
) -> Result<HG::OutputGadget, SynthesisError>
where
    F: PrimeField,
    CS: ConstraintSystem<F>,
    H: CRH,
    HG: MaskedCRHGadget<H, F>,
    TB: ToBytesGadget<F>,
{
    let bytes = leaf.to_bytes(&mut cs.ns(|| "left_to_bytes"))?;
    HG::check_evaluation_gadget_masked(cs, parameters, &bytes, &mask[..bytes.len() / 2])
}

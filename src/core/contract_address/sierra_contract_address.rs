use crate::{core::errors::contract_address_errors::ContractAddressError, EntryPointType};
use cairo_lang_starknet::{
    contract::starknet_keccak,
    contract_class::{ContractClass as SierraContractClass, ContractEntryPoint},
};
use cairo_vm::felt::Felt252;
use starknet_crypto::{poseidon_hash_many, FieldElement, PoseidonHasher};

const CONTRACT_CLASS_VERSION: &[u8] = b"CONTRACT_CLASS_V0.1.0";

// ---------------------------------
//  Version 2 functions and structs
// ---------------------------------

fn get_contract_entry_points_hashed(
    contract_class: &SierraContractClass,
    entry_point_type: &EntryPointType,
) -> Result<FieldElement, ContractAddressError> {
    let contract_entry_points = get_contract_entry_points(contract_class, entry_point_type)?;
    let mut hasher = PoseidonHasher::new();

    for entry_point in contract_entry_points {
        let selector =
            FieldElement::from_dec_str(&entry_point.selector.to_str_radix(10)).map_err(|_err| {
                ContractAddressError::Cast("String".to_string(), "FieldElement".to_string())
            })?;
        let function_idx = FieldElement::from(entry_point.function_idx);

        hasher.update(selector);
        hasher.update(function_idx);
    }

    Ok(hasher.finalize())
}

pub fn compute_sierra_class_hash(
    contract_class: &SierraContractClass,
) -> Result<Felt252, ContractAddressError> {
    let mut hasher = PoseidonHasher::new();

    // hash the API version
    let api_version = FieldElement::from_byte_slice_be(CONTRACT_CLASS_VERSION).map_err(|_err| {
        ContractAddressError::Cast("&[u8]".to_string(), "FieldElement".to_string())
    })?;

    hasher.update(api_version);

    // Entrypoints by type, hashed.
    let external_functions =
        get_contract_entry_points_hashed(contract_class, &EntryPointType::External)?;
    let l1_handlers = get_contract_entry_points_hashed(contract_class, &EntryPointType::L1Handler)?;
    let constructors =
        get_contract_entry_points_hashed(contract_class, &EntryPointType::Constructor)?;

    // hash the entrypoint hashes
    hasher.update(external_functions);
    hasher.update(l1_handlers);
    hasher.update(constructors);

    // Hash abi
    let abi = serde_json_pythonic::to_string_pythonic(
        &contract_class
            .abi
            .clone()
            .ok_or(ContractAddressError::MissingAbi)?
            .items,
    )
    .map_err(|_| ContractAddressError::MissingAbi)?;

    let abi_hash = FieldElement::from_byte_slice_be(&starknet_keccak(abi.as_bytes()).to_bytes_be())
        .map_err(|_err| {
            ContractAddressError::Cast("&[u8]".to_string(), "FieldElement".to_string())
        })?;

    hasher.update(abi_hash);

    let mut sierra_program_vector = Vec::with_capacity(contract_class.sierra_program.len());
    for number in &contract_class.sierra_program {
        let fe = FieldElement::from_dec_str(&number.value.to_str_radix(10)).map_err(|_err| {
            ContractAddressError::Cast("String".to_string(), "FieldElement".to_string())
        })?;
        sierra_program_vector.push(fe);
    }

    // Hash Sierra program.
    let sierra_program_ptr = poseidon_hash_many(&sierra_program_vector);

    hasher.update(sierra_program_ptr);
    let hash = hasher.finalize();
    Ok(Felt252::from_bytes_be(&hash.to_bytes_be()))
}

fn get_contract_entry_points(
    contract_class: &SierraContractClass,
    entry_point_type: &EntryPointType,
) -> Result<Vec<ContractEntryPoint>, ContractAddressError> {
    let program_length = contract_class.sierra_program.len();

    let entry_points = match entry_point_type {
        EntryPointType::Constructor => contract_class.entry_points_by_type.constructor.clone(),
        EntryPointType::External => contract_class.entry_points_by_type.external.clone(),
        EntryPointType::L1Handler => contract_class.entry_points_by_type.l1_handler.clone(),
    };

    for entry_point in &entry_points {
        if entry_point.function_idx > program_length {
            return Err(ContractAddressError::InvalidOffset(
                entry_point.function_idx,
            ));
        }
    }

    Ok(entry_points)
}

#[cfg(test)]
mod tests {
    use crate::core::contract_address::compute_sierra_class_hash;
    use cairo_lang_starknet::contract_class::ContractClass as SierraContractClass;
    use cairo_vm::felt::felt_str;
    use std::{fs::File, io::BufReader};

    #[test]
    fn test_declare_tx_from_testnet() {
        let file = File::open("starknet_programs/cairo2/events.sierra").unwrap();
        let reader = BufReader::new(file);

        let sierra_contract_class: SierraContractClass = serde_json::from_reader(reader).unwrap();

        // this is the class_hash from: https://alpha4.starknet.io/feeder_gateway/get_transaction?transactionHash=0x01b852f1fe2b13db21a44f8884bc4b7760dc277bb3820b970dba929860275617
        let expected_result = felt_str!(
            "487202222862199115032202787294865701687663153957776561394399544814644144883"
        );

        assert_eq!(
            compute_sierra_class_hash(&sierra_contract_class).unwrap(),
            expected_result
        )
    }
}

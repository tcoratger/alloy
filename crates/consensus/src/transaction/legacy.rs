use crate::{transaction::RlpEcdsaTx, SignableTransaction, Signed, Transaction, TxType};
use alloc::vec::Vec;
use alloy_eips::{eip2930::AccessList, eip7702::SignedAuthorization};
use alloy_primitives::{keccak256, Bytes, ChainId, Parity, Signature, TxKind, B256, U256};
use alloy_rlp::{length_of_length, BufMut, Decodable, Encodable, Header, Result};
use core::mem;

/// Enforce correct parity for legacy transactions (EIP-155, 27 or 28).
macro_rules! legacy_sig {
    ($signature:expr) => {
        if let Parity::Parity(parity) = $signature.v() {
            &$signature.with_parity(Parity::NonEip155(parity))
        } else {
            $signature
        }
    };
}

/// Legacy transaction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(any(test, feature = "arbitrary"), derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[doc(alias = "LegacyTransaction", alias = "TransactionLegacy", alias = "LegacyTx")]
pub struct TxLegacy {
    /// Added as EIP-155: Simple replay attack protection
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            with = "alloy_serde::quantity::opt",
            skip_serializing_if = "Option::is_none",
        )
    )]
    pub chain_id: Option<ChainId>,
    /// A scalar value equal to the number of transactions sent by the sender; formally Tn.
    #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity"))]
    pub nonce: u64,
    /// A scalar value equal to the number of
    /// Wei to be paid per unit of gas for all computation
    /// costs incurred as a result of the execution of this transaction; formally Tp.
    ///
    /// As ethereum circulation is around 120mil eth as of 2022 that is around
    /// 120000000000000000000000000 wei we are safe to use u128 as its max number is:
    /// 340282366920938463463374607431768211455
    #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity"))]
    pub gas_price: u128,
    /// A scalar value equal to the maximum
    /// amount of gas that should be used in executing
    /// this transaction. This is paid up-front, before any
    /// computation is done and may not be increased
    /// later; formally Tg.
    #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity", rename = "gas"))]
    pub gas_limit: u64,
    /// The 160-bit address of the message call’s recipient or, for a contract creation
    /// transaction, ∅, used here to denote the only member of B0 ; formally Tt.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "TxKind::is_create"))]
    pub to: TxKind,
    /// A scalar value equal to the number of Wei to
    /// be transferred to the message call’s recipient or,
    /// in the case of contract creation, as an endowment
    /// to the newly created account; formally Tv.
    pub value: U256,
    /// Input has two uses depending if transaction is Create or Call (if `to` field is None or
    /// Some). pub init: An unlimited size byte array specifying the
    /// EVM-code for the account initialisation procedure CREATE,
    /// data: An unlimited size byte array specifying the
    /// input data of the message call, formally Td.
    pub input: Bytes,
}

impl TxLegacy {
    /// The EIP-2718 transaction type.
    pub const TX_TYPE: isize = 0;

    /// Calculates a heuristic for the in-memory size of the [TxLegacy] transaction.
    #[inline]
    pub fn size(&self) -> usize {
        mem::size_of::<Option<ChainId>>() + // chain_id
        mem::size_of::<u64>() + // nonce
        mem::size_of::<u128>() + // gas_price
        mem::size_of::<u64>() + // gas_limit
        self.to.size() + // to
        mem::size_of::<U256>() + // value
        self.input.len() // input
    }

    /// Outputs the length of EIP-155 fields. Only outputs a non-zero value for EIP-155 legacy
    /// transactions.
    pub(crate) fn eip155_fields_len(&self) -> usize {
        self.chain_id.map_or(
            // this is either a pre-EIP-155 legacy transaction or a typed transaction
            0,
            // EIP-155 encodes the chain ID and two zeroes, so we add 2 to the length of the chain
            // ID to get the length of all 3 fields
            // len(chain_id) + (0x00) + (0x00)
            |id| id.length() + 2,
        )
    }

    /// Encodes EIP-155 arguments into the desired buffer. Only encodes values
    /// for legacy transactions.
    pub(crate) fn encode_eip155_signing_fields(&self, out: &mut dyn BufMut) {
        // if this is a legacy transaction without a chain ID, it must be pre-EIP-155
        // and does not need to encode the chain ID for the signature hash encoding
        if let Some(id) = self.chain_id {
            // EIP-155 encodes the chain ID and two zeroes
            id.encode(out);
            0x00u8.encode(out);
            0x00u8.encode(out);
        }
    }
}

// Legacy transaction network and 2718 encodings are identical to the RLP
// encoding.
impl RlpEcdsaTx for TxLegacy {
    const DEFAULT_TX_TYPE: u8 = { Self::TX_TYPE as u8 };

    fn rlp_encoded_fields_length(&self) -> usize {
        self.nonce.length()
            + self.gas_price.length()
            + self.gas_limit.length()
            + self.to.length()
            + self.value.length()
            + self.input.0.length()
    }

    fn rlp_encode_fields(&self, out: &mut dyn alloy_rlp::BufMut) {
        self.nonce.encode(out);
        self.gas_price.encode(out);
        self.gas_limit.encode(out);
        self.to.encode(out);
        self.value.encode(out);
        self.input.0.encode(out);
    }

    fn rlp_encoded_length_with_signature(&self, signature: &Signature) -> usize {
        // Enforce correct parity for legacy transactions (EIP-155, 27 or 28).
        let signature = legacy_sig!(signature);
        self.rlp_header_signed(signature).length_with_payload()
    }

    fn rlp_encode_signed(&self, signature: &Signature, out: &mut dyn BufMut) {
        // Enforce correct parity for legacy transactions (EIP-155, 27 or 28).
        let signature = legacy_sig!(signature);
        self.rlp_header_signed(signature).encode(out);
        self.rlp_encode_fields(out);
        signature.write_rlp_vrs(out);
    }

    fn eip2718_encoded_length(&self, signature: &Signature) -> usize {
        self.rlp_encoded_length_with_signature(signature)
    }

    fn eip2718_encode_with_type(&self, signature: &Signature, _ty: u8, out: &mut dyn BufMut) {
        self.rlp_encode_signed(signature, out);
    }

    fn network_encoded_length(&self, signature: &Signature) -> usize {
        self.rlp_encoded_length_with_signature(signature)
    }

    fn network_header(&self, signature: &Signature) -> Header {
        self.rlp_header_signed(signature)
    }

    fn network_encode_with_type(&self, signature: &Signature, _ty: u8, out: &mut dyn BufMut) {
        self.rlp_encode_signed(signature, out);
    }

    fn rlp_decode_fields(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        Ok(Self {
            nonce: Decodable::decode(buf)?,
            gas_price: Decodable::decode(buf)?,
            gas_limit: Decodable::decode(buf)?,
            to: Decodable::decode(buf)?,
            value: Decodable::decode(buf)?,
            input: Decodable::decode(buf)?,
            chain_id: None,
        })
    }

    fn rlp_decode_with_signature(buf: &mut &[u8]) -> alloy_rlp::Result<(Self, Signature)> {
        let header = Header::decode(buf)?;
        if !header.list {
            return Err(alloy_rlp::Error::UnexpectedString);
        }

        let remaining = buf.len();
        let mut tx = Self::rlp_decode_fields(buf)?;
        let signature = Signature::decode_rlp_vrs(buf)?;

        if !matches!(signature.v(), Parity::Eip155(_) | Parity::NonEip155(_)) {
            return Err(alloy_rlp::Error::Custom("invalid parity for legacy transaction"));
        }

        tx.chain_id = signature.v().chain_id();

        if buf.len() + header.payload_length != remaining {
            return Err(alloy_rlp::Error::ListLengthMismatch {
                expected: header.payload_length,
                got: remaining - buf.len(),
            });
        }

        Ok((tx, signature))
    }

    fn eip2718_decode_with_type(
        buf: &mut &[u8],
        _ty: u8,
    ) -> alloy_eips::eip2718::Eip2718Result<Signed<Self>> {
        Self::rlp_decode_signed(buf).map_err(Into::into)
    }

    fn eip2718_decode(buf: &mut &[u8]) -> alloy_eips::eip2718::Eip2718Result<Signed<Self>> {
        Self::rlp_decode_signed(buf).map_err(Into::into)
    }

    fn network_decode_with_type(
        buf: &mut &[u8],
        _ty: u8,
    ) -> alloy_eips::eip2718::Eip2718Result<Signed<Self>> {
        Self::rlp_decode_signed(buf).map_err(Into::into)
    }

    fn tx_hash_with_type(&self, signature: &Signature, _ty: u8) -> alloy_primitives::TxHash {
        let mut buf = Vec::with_capacity(self.rlp_encoded_length_with_signature(signature));
        self.rlp_encode_signed(signature, &mut buf);
        keccak256(&buf)
    }
}

impl Transaction for TxLegacy {
    fn chain_id(&self) -> Option<ChainId> {
        self.chain_id
    }

    fn nonce(&self) -> u64 {
        self.nonce
    }

    fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    fn gas_price(&self) -> Option<u128> {
        Some(self.gas_price)
    }

    fn max_fee_per_gas(&self) -> u128 {
        self.gas_price
    }

    fn max_priority_fee_per_gas(&self) -> Option<u128> {
        None
    }

    fn max_fee_per_blob_gas(&self) -> Option<u128> {
        None
    }

    fn priority_fee_or_price(&self) -> u128 {
        self.gas_price
    }

    fn kind(&self) -> TxKind {
        self.to
    }

    fn value(&self) -> U256 {
        self.value
    }

    fn input(&self) -> &Bytes {
        &self.input
    }

    fn ty(&self) -> u8 {
        TxType::Legacy as u8
    }

    fn access_list(&self) -> Option<&AccessList> {
        None
    }

    fn blob_versioned_hashes(&self) -> Option<&[B256]> {
        None
    }

    fn authorization_list(&self) -> Option<&[SignedAuthorization]> {
        None
    }
}

impl SignableTransaction<Signature> for TxLegacy {
    fn use_eip155(&self) -> bool {
        self.chain_id.is_some()
    }

    fn set_chain_id(&mut self, chain_id: ChainId) {
        self.chain_id = Some(chain_id);
    }

    fn encode_for_signing(&self, out: &mut dyn BufMut) {
        Header {
            list: true,
            payload_length: self.rlp_encoded_fields_length() + self.eip155_fields_len(),
        }
        .encode(out);
        self.rlp_encode_fields(out);
        self.encode_eip155_signing_fields(out);
    }

    fn payload_len_for_signature(&self) -> usize {
        let payload_length = self.rlp_encoded_fields_length() + self.eip155_fields_len();
        // 'header length' + 'payload length'
        Header { list: true, payload_length }.length_with_payload()
    }

    fn into_signed(self, signature: Signature) -> Signed<Self> {
        // Enforce correct parity for legacy transactions (EIP-155, 27 or 28).
        let signature = if let Parity::Parity(parity) = signature.v() {
            signature.with_parity(Parity::NonEip155(parity))
        } else {
            signature
        };

        let hash = self.tx_hash(&signature);
        Signed::new_unchecked(self, signature, hash)
    }
}

impl Encodable for TxLegacy {
    fn encode(&self, out: &mut dyn BufMut) {
        self.encode_for_signing(out)
    }

    fn length(&self) -> usize {
        let payload_length = self.rlp_encoded_fields_length() + self.eip155_fields_len();
        // 'header length' + 'payload length'
        length_of_length(payload_length) + payload_length
    }
}

impl Decodable for TxLegacy {
    fn decode(data: &mut &[u8]) -> Result<Self> {
        let header = Header::decode(data)?;
        let remaining_len = data.len();

        let transaction_payload_len = header.payload_length;

        if transaction_payload_len > remaining_len {
            return Err(alloy_rlp::Error::InputTooShort);
        }

        let mut transaction = Self::rlp_decode_fields(data)?;

        // If we still have data, it should be an eip-155 encoded chain_id
        if !data.is_empty() {
            transaction.chain_id = Some(Decodable::decode(data)?);
            let _: U256 = Decodable::decode(data)?; // r
            let _: U256 = Decodable::decode(data)?; // s
        }

        let decoded = remaining_len - data.len();
        if decoded != transaction_payload_len {
            return Err(alloy_rlp::Error::UnexpectedLength);
        }

        Ok(transaction)
    }
}

/// Bincode-compatible [`TxLegacy`] serde implementation.
#[cfg(all(feature = "serde", feature = "serde-bincode-compat"))]
pub(super) mod serde_bincode_compat {
    use alloc::borrow::Cow;
    use alloy_primitives::{Bytes, ChainId, TxKind, U256};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_with::{DeserializeAs, SerializeAs};

    /// Bincode-compatible [`super::TxLegacy`] serde implementation.
    ///
    /// Intended to use with the [`serde_with::serde_as`] macro in the following way:
    /// ```rust
    /// use alloy_consensus::{serde_bincode_compat, TxLegacy};
    /// use serde::{Deserialize, Serialize};
    /// use serde_with::serde_as;
    ///
    /// #[serde_as]
    /// #[derive(Serialize, Deserialize)]
    /// struct Data {
    ///     #[serde_as(as = "serde_bincode_compat::transaction::TxLegacy")]
    ///     header: TxLegacy,
    /// }
    /// ```
    #[derive(Debug, Serialize, Deserialize)]
    pub struct TxLegacy<'a> {
        #[serde(default, with = "alloy_serde::quantity::opt")]
        chain_id: Option<ChainId>,
        nonce: u64,
        gas_price: u128,
        gas_limit: u64,
        #[serde(default)]
        to: TxKind,
        value: U256,
        input: Cow<'a, Bytes>,
    }

    impl<'a> From<&'a super::TxLegacy> for TxLegacy<'a> {
        fn from(value: &'a super::TxLegacy) -> Self {
            Self {
                chain_id: value.chain_id,
                nonce: value.nonce,
                gas_price: value.gas_price,
                gas_limit: value.gas_limit,
                to: value.to,
                value: value.value,
                input: Cow::Borrowed(&value.input),
            }
        }
    }

    impl<'a> From<TxLegacy<'a>> for super::TxLegacy {
        fn from(value: TxLegacy<'a>) -> Self {
            Self {
                chain_id: value.chain_id,
                nonce: value.nonce,
                gas_price: value.gas_price,
                gas_limit: value.gas_limit,
                to: value.to,
                value: value.value,
                input: value.input.into_owned(),
            }
        }
    }

    impl SerializeAs<super::TxLegacy> for TxLegacy<'_> {
        fn serialize_as<S>(source: &super::TxLegacy, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            TxLegacy::from(source).serialize(serializer)
        }
    }

    impl<'de> DeserializeAs<'de, super::TxLegacy> for TxLegacy<'de> {
        fn deserialize_as<D>(deserializer: D) -> Result<super::TxLegacy, D::Error>
        where
            D: Deserializer<'de>,
        {
            TxLegacy::deserialize(deserializer).map(Into::into)
        }
    }

    #[cfg(test)]
    mod tests {
        use arbitrary::Arbitrary;
        use rand::Rng;
        use serde::{Deserialize, Serialize};
        use serde_with::serde_as;

        use super::super::{serde_bincode_compat, TxLegacy};

        #[test]
        fn test_tx_legacy_bincode_roundtrip() {
            #[serde_as]
            #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
            struct Data {
                #[serde_as(as = "serde_bincode_compat::TxLegacy")]
                transaction: TxLegacy,
            }

            let mut bytes = [0u8; 1024];
            rand::thread_rng().fill(bytes.as_mut_slice());
            let data = Data {
                transaction: TxLegacy::arbitrary(&mut arbitrary::Unstructured::new(&bytes))
                    .unwrap(),
            };

            let encoded = bincode::serialize(&data).unwrap();
            let decoded: Data = bincode::deserialize(&encoded).unwrap();
            assert_eq!(decoded, data);
        }
    }
}

#[cfg(all(test, feature = "k256"))]
mod tests {
    use crate::{SignableTransaction, TxLegacy};
    use alloy_primitives::{address, b256, hex, Address, Signature, TxKind, B256, U256};

    #[test]
    fn recover_signer_legacy() {
        let signer: Address = hex!("398137383b3d25c92898c656696e41950e47316b").into();
        let hash: B256 =
            hex!("bb3a336e3f823ec18197f1e13ee875700f08f03e2cab75f0d0b118dabb44cba0").into();

        let tx = TxLegacy {
            chain_id: Some(1),
            nonce: 0x18,
            gas_price: 0xfa56ea00,
            gas_limit: 119902,
            to: TxKind::Call(hex!("06012c8cf97bead5deae237070f9587f8e7a266d").into()),
            value: U256::from(0x1c6bf526340000u64),
            input:  hex!("f7d8c88300000000000000000000000000000000000000000000000000000000000cee6100000000000000000000000000000000000000000000000000000000000ac3e1").into(),
        };

        let sig = Signature::from_scalars_and_parity(
            b256!("2a378831cf81d99a3f06a18ae1b6ca366817ab4d88a70053c41d7a8f0368e031"),
            b256!("450d831a05b6e418724436c05c155e0a1b7b921015d0fbc2f667aed709ac4fb5"),
            37,
        )
        .unwrap();

        let signed_tx = tx.into_signed(sig);

        assert_eq!(*signed_tx.hash(), hash, "Expected same hash");
        assert_eq!(signed_tx.recover_signer().unwrap(), signer, "Recovering signer should pass.");
    }

    #[test]
    // Test vector from https://github.com/alloy-rs/alloy/issues/125
    fn decode_legacy_and_recover_signer() {
        use crate::transaction::RlpEcdsaTx;
        let raw_tx = alloy_primitives::bytes!("f9015482078b8505d21dba0083022ef1947a250d5630b4cf539739df2c5dacb4c659f2488d880c46549a521b13d8b8e47ff36ab50000000000000000000000000000000000000000000066ab5a608bd00a23f2fe000000000000000000000000000000000000000000000000000000000000008000000000000000000000000048c04ed5691981c42154c6167398f95e8f38a7ff00000000000000000000000000000000000000000000000000000000632ceac70000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000006c6ee5e31d828de241282b9606c8e98ea48526e225a0c9077369501641a92ef7399ff81c21639ed4fd8fc69cb793cfa1dbfab342e10aa0615facb2f1bcf3274a354cfe384a38d0cc008a11c2dd23a69111bc6930ba27a8");

        let tx = TxLegacy::rlp_decode_signed(&mut raw_tx.as_ref()).unwrap();

        let recovered = tx.recover_signer().unwrap();
        let expected = address!("a12e1462d0ceD572f396F58B6E2D03894cD7C8a4");

        assert_eq!(tx.tx().chain_id, Some(1), "Expected same chain id");
        assert_eq!(expected, recovered, "Expected same signer");
    }
}

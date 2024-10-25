use crate::receipt::{Eip658Value, TxReceipt};
use alloc::{vec, vec::Vec};
use alloy_primitives::{Bloom, Log, B256};
use alloy_rlp::{length_of_length, BufMut, Decodable, Encodable};
use core::{borrow::Borrow, fmt};
use derive_more::{DerefMut, From, IntoIterator};

/// Receipt containing result of transaction execution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(any(test, feature = "arbitrary"), derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[doc(alias = "TransactionReceipt", alias = "TxReceipt")]
pub struct Receipt<T = Log> {
    /// If transaction is executed successfully.
    ///
    /// This is the `statusCode`
    #[cfg_attr(feature = "serde", serde(alias = "root"))]
    pub status: Eip658Value,
    /// Gas used
    #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity"))]
    pub cumulative_gas_used: u128,
    /// Log send from contracts.
    pub logs: Vec<T>,
}

#[cfg(feature = "serde")]
impl<T> serde::Serialize for Receipt<T>
where
    T: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut s = serializer.serialize_struct("Receipt", 3)?;

        // If the status is EIP-658, serialize the status field.
        // Otherwise, serialize the root field.
        let key = if self.status.is_eip658() { "status" } else { "root" };
        s.serialize_field(key, &self.status)?;

        s.serialize_field(
            "cumulativeGasUsed",
            &alloy_primitives::U128::from(self.cumulative_gas_used),
        )?;
        s.serialize_field("logs", &self.logs)?;

        s.end()
    }
}

impl<T> Receipt<T>
where
    T: Borrow<Log>,
{
    /// Calculates [`Log`]'s bloom filter. this is slow operation and [ReceiptWithBloom] can
    /// be used to cache this value.
    pub fn bloom_slow(&self) -> Bloom {
        self.logs.iter().map(Borrow::borrow).collect()
    }

    /// Calculates the bloom filter for the receipt and returns the [ReceiptWithBloom] container
    /// type.
    pub fn with_bloom(self) -> ReceiptWithBloom<T> {
        self.into()
    }
}

impl<T> TxReceipt<T> for Receipt<T>
where
    T: Borrow<Log> + Clone + fmt::Debug + PartialEq + Eq + Send + Sync,
{
    fn status_or_post_state(&self) -> Eip658Value {
        self.status
    }

    fn status(&self) -> bool {
        self.status.coerce_status()
    }

    fn bloom(&self) -> Bloom {
        self.bloom_slow()
    }

    fn cumulative_gas_used(&self) -> u128 {
        self.cumulative_gas_used
    }

    fn logs(&self) -> &[T] {
        &self.logs
    }
}

impl<T> From<ReceiptWithBloom<T>> for Receipt<T> {
    /// Consume the structure, returning only the receipt
    fn from(receipt_with_bloom: ReceiptWithBloom<T>) -> Self {
        receipt_with_bloom.receipt
    }
}

/// Receipt containing result of transaction execution.
#[derive(
    Clone, Debug, PartialEq, Eq, Default, From, derive_more::Deref, DerefMut, IntoIterator,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Receipts<T> {
    /// A two-dimensional vector of [`Receipt`] instances.
    pub receipt_vec: Vec<Vec<T>>,
}

impl<T> Receipts<T> {
    /// Returns the length of the [`Receipts`] vector.
    pub fn len(&self) -> usize {
        self.receipt_vec.len()
    }

    /// Returns `true` if the [`Receipts`] vector is empty.
    pub fn is_empty(&self) -> bool {
        self.receipt_vec.is_empty()
    }

    /// Push a new vector of receipts into the [`Receipts`] collection.
    pub fn push(&mut self, receipts: Vec<T>) {
        self.receipt_vec.push(receipts);
    }

    /// Retrieves all recorded receipts from index and calculates the root using the given closure.
    pub fn root_slow(&self, index: usize, f: impl FnOnce(&[&T]) -> B256) -> Option<B256> {
        self.receipt_vec.get(index).map(|receipts| f(&receipts.iter().collect::<Vec<_>>()))
    }
}

impl<T> From<Vec<T>> for Receipts<T> {
    fn from(block_receipts: Vec<T>) -> Self {
        Self { receipt_vec: vec![block_receipts] }
    }
}

impl<T> FromIterator<Vec<T>> for Receipts<T> {
    fn from_iter<I: IntoIterator<Item = Vec<T>>>(iter: I) -> Self {
        Self { receipt_vec: iter.into_iter().collect() }
    }
}

/// [`Receipt`] with calculated bloom filter.
///
/// This convenience type allows us to lazily calculate the bloom filter for a
/// receipt, similar to [`Sealed`].
///
/// [`Sealed`]: crate::Sealed
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
#[doc(alias = "TransactionReceiptWithBloom", alias = "TxReceiptWithBloom")]
pub struct ReceiptWithBloom<T = Log> {
    #[cfg_attr(feature = "serde", serde(flatten))]
    /// The receipt.
    pub receipt: Receipt<T>,
    /// The bloom filter.
    pub logs_bloom: Bloom,
}

impl<T> TxReceipt<T> for ReceiptWithBloom<T>
where
    T: Clone + fmt::Debug + PartialEq + Eq + Send + Sync,
{
    fn status_or_post_state(&self) -> Eip658Value {
        self.receipt.status
    }

    fn status(&self) -> bool {
        matches!(self.receipt.status, Eip658Value::Eip658(true) | Eip658Value::PostState(_))
    }

    fn bloom(&self) -> Bloom {
        self.logs_bloom
    }

    fn bloom_cheap(&self) -> Option<Bloom> {
        Some(self.logs_bloom)
    }

    fn cumulative_gas_used(&self) -> u128 {
        self.receipt.cumulative_gas_used
    }

    fn logs(&self) -> &[T] {
        &self.receipt.logs
    }
}

impl<T> From<Receipt<T>> for ReceiptWithBloom<T>
where
    T: Borrow<Log>,
{
    fn from(receipt: Receipt<T>) -> Self {
        let bloom = receipt.bloom_slow();
        Self { receipt, logs_bloom: bloom }
    }
}

impl<T: Encodable> ReceiptWithBloom<T> {
    /// Returns the rlp header for the receipt payload.
    fn receipt_rlp_header(&self) -> alloy_rlp::Header {
        alloy_rlp::Header { list: true, payload_length: self.payload_len() }
    }

    /// Encodes the receipt data.
    fn encode_fields(&self, out: &mut dyn BufMut) {
        self.receipt_rlp_header().encode(out);
        self.receipt.status.encode(out);
        self.receipt.cumulative_gas_used.encode(out);
        self.logs_bloom.encode(out);
        self.receipt.logs.encode(out);
    }

    fn payload_len(&self) -> usize {
        self.receipt.status.length()
            + self.receipt.cumulative_gas_used.length()
            + self.logs_bloom.length()
            + self.receipt.logs.length()
    }
}

impl<T> ReceiptWithBloom<T> {
    /// Create new [ReceiptWithBloom]
    pub const fn new(receipt: Receipt<T>, logs_bloom: Bloom) -> Self {
        Self { receipt, logs_bloom }
    }

    /// Consume the structure, returning the receipt and the bloom filter
    pub fn into_components(self) -> (Receipt<T>, Bloom) {
        (self.receipt, self.logs_bloom)
    }

    /// Decodes the receipt payload
    fn decode_receipt(buf: &mut &[u8]) -> alloy_rlp::Result<Self>
    where
        T: Decodable,
    {
        let b: &mut &[u8] = &mut &**buf;
        let rlp_head = alloy_rlp::Header::decode(b)?;
        if !rlp_head.list {
            return Err(alloy_rlp::Error::UnexpectedString);
        }
        let started_len = b.len();

        let success = Decodable::decode(b)?;
        let cumulative_gas_used = Decodable::decode(b)?;
        let bloom = Decodable::decode(b)?;
        let logs = Decodable::decode(b)?;

        let receipt = Receipt { status: success, cumulative_gas_used, logs };

        let this = Self { receipt, logs_bloom: bloom };
        let consumed = started_len - b.len();
        if consumed != rlp_head.payload_length {
            return Err(alloy_rlp::Error::ListLengthMismatch {
                expected: rlp_head.payload_length,
                got: consumed,
            });
        }
        *buf = *b;
        Ok(this)
    }
}

impl<T: Encodable> Encodable for ReceiptWithBloom<T> {
    fn encode(&self, out: &mut dyn BufMut) {
        self.encode_fields(out);
    }

    fn length(&self) -> usize {
        let payload_length = self.receipt.status.length()
            + self.receipt.cumulative_gas_used.length()
            + self.logs_bloom.length()
            + self.receipt.logs.length();
        payload_length + length_of_length(payload_length)
    }
}

impl<T: Decodable> Decodable for ReceiptWithBloom<T> {
    fn decode(buf: &mut &[u8]) -> alloy_rlp::Result<Self> {
        Self::decode_receipt(buf)
    }
}

#[cfg(any(test, feature = "arbitrary"))]
impl<'a, T> arbitrary::Arbitrary<'a> for ReceiptWithBloom<T>
where
    T: arbitrary::Arbitrary<'a>,
{
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        Ok(Self { receipt: Receipt::<T>::arbitrary(u)?, logs_bloom: Bloom::arbitrary(u)? })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[cfg(feature = "serde")]
    #[test]
    fn root_vs_status() {
        let receipt = super::Receipt::<()> {
            status: super::Eip658Value::Eip658(true),
            cumulative_gas_used: 0,
            logs: Vec::new(),
        };

        let json = serde_json::to_string(&receipt).unwrap();
        assert_eq!(json, r#"{"status":"0x1","cumulativeGasUsed":"0x0","logs":[]}"#);

        let receipt = super::Receipt::<()> {
            status: super::Eip658Value::PostState(Default::default()),
            cumulative_gas_used: 0,
            logs: Vec::new(),
        };

        let json = serde_json::to_string(&receipt).unwrap();
        assert_eq!(
            json,
            r#"{"root":"0x0000000000000000000000000000000000000000000000000000000000000000","cumulativeGasUsed":"0x0","logs":[]}"#
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn deser_pre658() {
        use alloy_primitives::b256;

        let json = r#"{"root":"0x284d35bf53b82ef480ab4208527325477439c64fb90ef518450f05ee151c8e10","cumulativeGasUsed":"0x0","logs":[]}"#;

        let receipt: super::Receipt<()> = serde_json::from_str(json).unwrap();

        assert_eq!(
            receipt.status,
            super::Eip658Value::PostState(b256!(
                "284d35bf53b82ef480ab4208527325477439c64fb90ef518450f05ee151c8e10"
            ))
        );
    }

    #[test]
    fn test_root_slow_valid_index() {
        // Create a first dummy receipt
        let receipt1: Receipt<Log> =
            Receipt { status: Eip658Value::Eip658(true), cumulative_gas_used: 1000, logs: vec![] };

        // Create a second dummy receipt
        let receipt2 =
            Receipt { status: Eip658Value::Eip658(false), cumulative_gas_used: 2000, logs: vec![] };

        // Create a `Receipts` instance with a single set of receipts
        let receipts = Receipts { receipt_vec: vec![vec![receipt1.clone(), receipt2.clone()]] };

        // Calculate the root hash of the receipts at index 0
        //
        // We are using a dummy closure that generates a root hash based on the number of receipts.
        let root = receipts.root_slow(0, |receipts| B256::with_last_byte(receipts.len() as u8));

        // Verify that the calculated root matches the expected result (encoded with 2 receipts)
        assert_eq!(root, Some(B256::with_last_byte(2)));
    }

    #[test]
    fn test_root_slow_empty_receipts() {
        // Initialize `Receipts` with an empty receipt set
        let receipts = Receipts::<Receipt> { receipt_vec: vec![vec![]] };

        // Call `root_slow` with index 0 and provide the `calculate_root` closure.
        //
        // Since there are no receipts, it should calculate a root based on 0 receipts.
        let root = receipts.root_slow(0, |receipts| B256::with_last_byte(receipts.len() as u8));

        // Assert that the root is calculated correctly (0 receipts)
        assert_eq!(root, Some(B256::with_last_byte(0)));
    }

    #[test]
    fn test_root_slow_invalid_index() {
        // Create a sample receipt for testing
        let receipt: Receipt<Log> =
            Receipt { status: Eip658Value::Eip658(true), cumulative_gas_used: 1000, logs: vec![] };

        // Initialize `Receipts` with a single set of receipts
        let receipts = Receipts { receipt_vec: vec![vec![receipt.clone()]] };

        // Calculate the root hash of the receipts at index 1 (invalid index)
        let root = receipts.root_slow(1, |receipts| B256::with_last_byte(receipts.len() as u8));

        // Assert that `root` is `None` for an invalid index
        assert!(root.is_none());
    }

    #[test]
    fn test_root_slow_multiple_receipt_sets() {
        // Create multiple dummy receipts
        let receipt1: Receipt<Log> =
            Receipt { status: Eip658Value::Eip658(true), cumulative_gas_used: 1000, logs: vec![] };
        let receipt2 =
            Receipt { status: Eip658Value::Eip658(false), cumulative_gas_used: 2000, logs: vec![] };
        let receipt3 =
            Receipt { status: Eip658Value::Eip658(true), cumulative_gas_used: 3000, logs: vec![] };

        // Initialize `Receipts` with two sets of receipts, each containing a different count
        let receipts = Receipts {
            receipt_vec: vec![vec![receipt1.clone()], vec![receipt2.clone(), receipt3.clone()]],
        };

        // Calculate root for the first set (index 0) using `calculate_root`
        let root_set_0 =
            receipts.root_slow(0, |receipts| B256::with_last_byte(receipts.len() as u8));

        // Confirm that the root for the first set matches the count of 1 receipt
        assert_eq!(root_set_0, Some(B256::with_last_byte(1)));

        // Calculate root for the second set (index 1), which has 2 receipts
        let root_set_1 =
            receipts.root_slow(1, |receipts| B256::with_last_byte(receipts.len() as u8));

        // Verify the root for the second set matches the count of 2 receipts
        assert_eq!(root_set_1, Some(B256::with_last_byte(2)));
    }
}

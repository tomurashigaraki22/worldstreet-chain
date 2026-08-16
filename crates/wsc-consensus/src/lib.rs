use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use wsc_core::{canonical_encode, Hash, PublicKey, Signature, Validator, CHAIN_ID};
use wsc_crypto::KeyPair;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vote {
    pub version: u8,
    pub chain_id: String,
    pub height: u64,
    pub round: u64,
    pub block_hash: Hash,
    pub validator: PublicKey,
    pub signature: Signature,
}

impl Vote {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, ConsensusError> {
        let mut unsigned = self.clone();
        unsigned.signature = Signature([0; 64]);
        canonical_encode(&unsigned).map_err(|error| ConsensusError::Encoding(error.to_string()))
    }

    pub fn sign(
        chain_id: impl Into<String>,
        height: u64,
        round: u64,
        block_hash: Hash,
        validator: &KeyPair,
    ) -> Result<Self, ConsensusError> {
        let mut vote = Self {
            version: 1,
            chain_id: chain_id.into(),
            height,
            round,
            block_hash,
            validator: validator.public_key(),
            signature: Signature([0; 64]),
        };
        let bytes = vote.signing_bytes()?;
        vote.signature = validator.sign(&bytes);
        Ok(vote)
    }

    pub fn verify(&self) -> Result<(), ConsensusError> {
        if self.version != 1 {
            return Err(ConsensusError::UnsupportedVersion);
        }
        let bytes = self.signing_bytes()?;
        if !KeyPair::verify(&self.validator, &bytes, &self.signature) {
            return Err(ConsensusError::InvalidSignature);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ValidatorSet {
    validators: BTreeMap<PublicKey, Validator>,
}

impl ValidatorSet {
    pub fn new(validators: impl IntoIterator<Item = Validator>) -> Self {
        Self {
            validators: validators
                .into_iter()
                .map(|validator| (validator.public_key, validator))
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.validators.len()
    }

    pub fn contains(&self, key: &PublicKey) -> bool {
        self.validators.contains_key(key)
    }

    pub fn is_quorum(&self, votes: usize) -> bool {
        self.len() > 0 && votes.saturating_mul(3) >= self.len().saturating_mul(2)
    }

    pub fn validators(&self) -> impl Iterator<Item = &Validator> {
        self.validators.values()
    }

    pub fn validate_vote(
        &self,
        vote: &Vote,
        chain_id: &str,
        height: u64,
        round: u64,
        block_hash: Hash,
    ) -> Result<(), ConsensusError> {
        if vote.chain_id != chain_id {
            return Err(ConsensusError::WrongChainId);
        }
        if vote.height != height {
            return Err(ConsensusError::WrongHeight);
        }
        if vote.round != round {
            return Err(ConsensusError::WrongRound);
        }
        if vote.block_hash != block_hash {
            return Err(ConsensusError::WrongBlock);
        }
        if !self.contains(&vote.validator) {
            return Err(ConsensusError::UnknownValidator);
        }
        vote.verify()
    }
}

#[derive(Clone, Debug)]
pub struct VoteSet {
    pub chain_id: String,
    pub height: u64,
    pub round: u64,
    pub block_hash: Hash,
    votes: BTreeMap<PublicKey, Vote>,
}

#[derive(Debug, Error)]
pub enum ConsensusError {
    #[error("unsupported consensus version")]
    UnsupportedVersion,
    #[error("wrong chain ID")]
    WrongChainId,
    #[error("wrong height")]
    WrongHeight,
    #[error("wrong round")]
    WrongRound,
    #[error("wrong block hash")]
    WrongBlock,
    #[error("unknown validator")]
    UnknownValidator,
    #[error("invalid vote signature")]
    InvalidSignature,
    #[error("duplicate validator vote")]
    DuplicateVote,
    #[error("encoding error: {0}")]
    Encoding(String),
    #[error("no validator quorum")]
    NoQuorum,
}

impl VoteSet {
    pub fn new(chain_id: impl Into<String>, height: u64, round: u64, block_hash: Hash) -> Self {
        Self {
            chain_id: chain_id.into(),
            height,
            round,
            block_hash,
            votes: BTreeMap::new(),
        }
    }

    pub fn record(
        &mut self,
        validator_set: &ValidatorSet,
        vote: Vote,
    ) -> Result<bool, ConsensusError> {
        validator_set.validate_vote(
            &vote,
            &self.chain_id,
            self.height,
            self.round,
            self.block_hash,
        )?;
        if self.votes.contains_key(&vote.validator) {
            return Err(ConsensusError::DuplicateVote);
        }
        self.votes.insert(vote.validator, vote);
        Ok(validator_set.is_quorum(self.votes.len()))
    }

    pub fn len(&self) -> usize {
        self.votes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.votes.is_empty()
    }

    pub fn is_quorum(&self, validator_set: &ValidatorSet) -> bool {
        validator_set.is_quorum(self.votes.len())
    }

    pub fn votes(&self) -> impl Iterator<Item = &Vote> {
        self.votes.values()
    }
}

pub fn proposer_for_height(
    validators: &[Validator],
    height: u64,
    round: u64,
) -> Option<&Validator> {
    if validators.is_empty() {
        return None;
    }
    let index = (height as usize + round as usize) % validators.len();
    validators.get(index)
}

pub fn default_chain_id() -> &'static str {
    CHAIN_ID
}

#[cfg(test)]
mod tests {
    use super::*;
    use wsc_core::Validator;

    fn validator(name: &str) -> (KeyPair, Validator) {
        let key = KeyPair::generate().unwrap();
        let validator = Validator {
            name: name.to_owned(),
            public_key: key.public_key(),
        };
        (key, validator)
    }

    #[test]
    fn two_of_three_votes_reach_quorum() {
        let (key_a, validator_a) = validator("a");
        let (key_b, validator_b) = validator("b");
        let (_key_c, validator_c) = validator("c");
        let set = ValidatorSet::new([validator_a.clone(), validator_b.clone(), validator_c]);
        let hash = Hash([9; 32]);
        let mut votes = VoteSet::new(CHAIN_ID, 4, 0, hash);

        assert!(!votes
            .record(&set, Vote::sign(CHAIN_ID, 4, 0, hash, &key_a).unwrap())
            .unwrap());
        assert!(votes
            .record(&set, Vote::sign(CHAIN_ID, 4, 0, hash, &key_b).unwrap())
            .unwrap());
        assert_eq!(votes.len(), 2);
    }

    #[test]
    fn unknown_validator_is_rejected() {
        let (_known_key, known) = validator("known");
        let (unknown_key, _) = validator("unknown");
        let set = ValidatorSet::new([known]);
        let vote = Vote::sign(CHAIN_ID, 1, 0, Hash([1; 32]), &unknown_key).unwrap();
        assert!(matches!(
            set.validate_vote(&vote, CHAIN_ID, 1, 0, Hash([1; 32])),
            Err(ConsensusError::UnknownValidator)
        ));
    }
}

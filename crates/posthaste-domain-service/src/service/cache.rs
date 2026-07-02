use std::collections::HashSet;
use std::time::Duration;

use posthaste_observability::{events, ph_debug, ph_trace};

use crate::cache::{decide_cache_admission, score_cache_candidate, CacheAdmission};
use crate::MailGateway;
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, CacheCandidate, CacheCandidateSignals,
    CacheFetchLease, CacheFetchUnit, CacheLayer, CacheMessageSignals, CacheObjectState,
    CachePolicy, CachePriorityUpdate, CacheRescoreBatchOutcome, CacheRescoreCandidate,
    CacheSignalUpdate, CacheWorkerBatchOutcome, MessageId, MessagePage, MessageRecord,
    ServiceError, StoreError,
};

use super::MailService;

mod body_worker;
mod candidates;
mod helpers;
mod rescore;
mod visibility;

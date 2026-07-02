use std::collections::HashSet;
use std::time::Duration;

use posthaste_observability::{events, ph_debug, ph_trace};

use crate::{
    decide_cache_admission, score_cache_candidate, AccountDriver, AccountId, AccountSettings,
    CacheAdmission, CacheCandidate, CacheCandidateSignals, CacheFetchLease, CacheFetchUnit,
    CacheLayer, CacheMessageSignals, CacheObjectState, CachePolicy, CachePriorityUpdate,
    CacheRescoreBatchOutcome, CacheRescoreCandidate, CacheSignalUpdate, CacheWorkerBatchOutcome,
    MailGateway, MessageId, MessagePage, MessageRecord, ServiceError, StoreError,
};

use super::MailService;

mod body_worker;
mod candidates;
mod helpers;
mod rescore;
mod visibility;

/// The BASE-plane write capability (NS1b, RFC-L2-client-replication-model
/// D165): every port method that mutates base — the sync-owned provider-truth
/// tables — demands a `&BaseWrite` witness, so "who may write base" is a
/// compile-time property, not a review convention.
///
/// The tuple field is private to this module: nothing outside it can construct
/// the type directly. The two constructors define the whole policy:
///
/// - [`BaseWrite::reconciler`] — `pub(crate)`: only `posthaste-domain-service`
///   (which owns the reconciler role — the sync sink, the settlement-readback
///   write, the lazy body persist) can authorize a base write as provider
///   truth. The store, the authority server, the runtime, bench, testkit, and
///   every other crate CANNOT mint this; the compiler rejects the call.
/// - [`BaseWrite::legacy`] — `pub` and LOUD: the escape hatch for the named
///   non-reconciler writers that predate their own cutover, and for test/bench
///   seeding (a test legitimately plays the reconciler when it seeds base).
///   `rg 'BaseWrite::legacy'` IS the remaining-violation inventory; production
///   grants must shrink to zero as their cutovers land (draft-discard → NS2).
///
/// Zero-sized: no runtime cost, erased at codegen. The seal exists so the
/// one-writer invariant cannot be silently violated by a future change (or a
/// future agent) — an unauthorized base write is a compile error, not a
/// review finding.
pub struct BaseWrite(());

impl BaseWrite {
    /// The reconciler role's witness: base is being written with PROVIDER
    /// TRUTH (a sync batch, a settlement readback, a fetched body).
    pub(crate) fn reconciler() -> Self {
        Self(())
    }

    /// A named, greppable exception. `reason` documents who and why at the
    /// call site; it is deliberately `&'static str` so grants are literal,
    /// searchable strings — never computed, never forwarded.
    pub fn legacy(reason: &'static str) -> Self {
        let _ = reason;
        Self(())
    }
}

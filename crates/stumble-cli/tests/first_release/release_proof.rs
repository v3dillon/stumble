use crate::common::{
    assert_adapter_parity, assert_home_public_exports_are_private, canonical_feed,
};
use crate::scenario::*;
use chrono::Utc;
use stumble_core::FeedBatchRequest;

#[tokio::test]
async fn scoped_harness_proves_the_complete_headless_two_node_first_release() {
    let scenario = arrange_two_node_scenario();
    let now = Utc::now();
    let discovery = discover_and_curate_local_content(&scenario, now);
    deliver_local_item_for_old_gem(&scenario, &discovery, now);
    let federation = establish_federation(&scenario, now).await;
    let feed_mix = arrange_feed_mix_evidence(&scenario, now);
    let composition =
        prove_complete_feed_composition(&scenario, &discovery, &federation, &feed_mix, now);
    let ranked =
        apply_feedback_and_prove_reranking(&scenario, &discovery, &federation, &composition, now);
    prove_unavailable_category_backfill(&scenario, &ranked, now);
    withdraw_and_synchronize_origin_placement(&scenario, &federation, now).await;
    assert_home_public_exports_are_private(
        &scenario.home,
        &[
            "private feedback needle",
            "Scoped unattended discovery worker private needle",
            "Interactive Feed operator private needle",
            scenario.worker_token.as_str(),
            &discovery.task.id.to_string(),
        ],
    )
    .await;
    // Only placement arrays are canonicalized; ranked Feed item order remains contractual.
    let adapter_expected = canonical_feed(
        serde_json::to_value(
            scenario
                .home
                .get_feed_batch(&scenario.user, FeedBatchRequest::new(2).unwrap(), now)
                .unwrap(),
        )
        .unwrap(),
    );
    assert_adapter_parity(&scenario.home_dir, &scenario.user_token, &adapter_expected).await;
    prove_restart(&scenario, &federation);
}

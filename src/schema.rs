table! {
    review_requests (id) {
        id -> Integer,
        project -> Text,
        pr_number -> Text,
        pr_url -> Text,
        created_at -> Timestamp,
    }
}

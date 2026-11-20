//! schema_v78: every local GGUF chat model starts on the automatic context (0), other providers
//! keep theirs; the chat's search-call budget setting is gone. schema_v79: the tables' one too.

use super::llm_endpoint_tests::seed_endpoint;
use super::Database;

#[test]
fn local_models_start_on_automatic_and_the_chat_budget_setting_goes() {
    let db = Database::open_in_memory().unwrap();
    let local = seed_endpoint(&db, "Local", "gguf");
    let hosted = seed_endpoint(&db, "Hosted", "openai");
    db.set_setting("chat_tool_call_budget_default", &5).unwrap();
    db.set_setting("grid_tool_call_budget_default", &4).unwrap();
    // v78 and v79 run again over the rows above, as on an existing install (later migrations are
    // not idempotent, so only these two).
    db.conn.execute_batch(super::MIGRATIONS[77]).unwrap();
    db.conn.execute_batch(super::MIGRATIONS[78]).unwrap();

    let window = |id| db.llm_endpoint(id).unwrap().unwrap().context_window;
    assert_eq!((window(local), window(hosted)), (0, 8192));
    assert_eq!(db.get_setting::<i64>("chat_tool_call_budget_default").unwrap(), None);
    assert_eq!(db.get_setting::<i64>("grid_tool_call_budget_default").unwrap(), None);
}

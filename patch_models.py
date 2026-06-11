import re

with open("/Users/matthias.nordwig/matrix/core/src/db/models.rs", "r") as f:
    text = f.read()

pattern_context = re.compile(r'(pub struct Context \{.*?embedding_dim: Option<i64>,)(.*?status: ContextStatus,)', re.DOTALL)
text = pattern_context.sub(r'\1\n    pub chunk_endpoint_id: Option<i64>,\2', text)

pattern_new_context = re.compile(r'(pub struct NewContext \{.*?embedding_dim: Option<i64>,)(.*?\})', re.DOTALL)
text = pattern_new_context.sub(r'\1\n    pub chunk_endpoint_id: Option<i64>,\2', text)

with open("/Users/matthias.nordwig/matrix/core/src/db/models.rs", "w") as f:
    f.write(text)

print("models.rs patched")

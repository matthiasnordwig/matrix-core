import re

with open("/Users/matthias.nordwig/matrix/app/src/ipc.ts", "r") as f:
    text = f.read()

# Context
text = text.replace('  embedding_dim: number | null;', '  embedding_dim: number | null;\n  chunk_endpoint_id: number | null;')

with open("/Users/matthias.nordwig/matrix/app/src/ipc.ts", "w") as f:
    f.write(text)

print("ipc.ts patched")

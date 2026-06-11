with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

old_save = """      chunking_strategy: chunkingStrategy,
      chunking_profile_id: chunkingStrategy === "prompt" ? profileId : null,
      structural_profile_id: chunkingStrategy === "structural" ? structuralProfileId : null,
      embedding_model_id: modelId,
      embedding_dim: model ? model.default_dim : null,
    };"""

new_save = """      chunking_strategy: chunkingStrategy,
      chunking_profile_id: chunkingStrategy === "prompt" ? profileId : null,
      structural_profile_id: chunkingStrategy === "structural" ? structuralProfileId : null,
      embedding_model_id: modelId,
      embedding_dim: model ? model.default_dim : null,
      chunk_endpoint_id: chunkingStrategy === "prompt" ? chunkEndpointId : null,
    };"""
text = text.replace(old_save, new_save)

# Fix NewContext in ipc.ts
old_ipc = "  embedding_dim: number | null;\n  chunk_endpoint_id: number | null;"
new_ipc = "  embedding_dim: number | null;\n  chunk_endpoint_id: number | null;\n}\n\nexport interface NewContext {\n  name: string;\n  description: string | null;\n  chunking_strategy: string;\n  chunking_profile_id: number | null;\n  structural_profile_id: number | null;\n  embedding_model_id: number | null;\n  embedding_dim: number | null;\n  chunk_endpoint_id: number | null;"

with open("/Users/matthias.nordwig/matrix/app/src/ipc.ts", "r") as f:
    ipc_text = f.read()

ipc_text = ipc_text.replace("  embedding_dim: number | null;\n}", "  embedding_dim: number | null;\n  chunk_endpoint_id: number | null;\n}")
with open("/Users/matthias.nordwig/matrix/app/src/ipc.ts", "w") as f:
    f.write(ipc_text)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("fixed")

import re

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Replace the chunk with label inside ctx-head
pattern = re.compile(r'        <label className="inline">Chunk with:.*?</label>\s*<span className="fill" />', re.DOTALL)
text = pattern.sub(r'        <span className="fill" />', text)

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("fixed ctx-head")

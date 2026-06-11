with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "r") as f:
    text = f.read()

# Replace the old `openAdd` with nothing, and rename `startAdd` to `openAdd`
text = text.replace('  const openAdd = () => { cancel(); setPasteFor(null); setAdding(true); };\n', '')
text = text.replace('  const startAdd = () => {', '  const openAdd = () => {\n    setPasteFor(null);')
text = text.replace('startAdd();', 'openAdd();')

with open("/Users/matthias.nordwig/matrix/app/src/pages.tsx", "w") as f:
    f.write(text)

print("done")

<div align="center">

# Jango

**Inspired by Django and implemented in TypeScript**

[📚 Docs](https://uoxone.github.io/jango/) · [🌿 Start](https://uoxone.github.io/jango/start.html) · [🧭 Routes](https://uoxone.github.io/jango/routes.html) · [🧪 Tests](https://uoxone.github.io/jango/test.html)

</div>

---

## 🌿 Install the command

```bash
bun install -g @uoxone/jango
# or
npm install -g @uoxone/jango
```

## 🌿 Link the command

If you download the repository from GitHub, use these commands instead:

```bash
git clone https://github.com/uoxone/Jango.git
cd Jango
bun link
```

`bun link` takes no package name. It registers this folder and installs the `jango` command.

From this directory, remove that link:

```bash
bun unlink
```

## ✨ Create and run

```bash
jango create project mehrashop
jango run mehrashop
```

The server is at `http://127.0.0.1:8000`.

## 📚 Docs

The full guide is at [uoxone.github.io/jango](https://uoxone.github.io/jango/).

## 🚀 Git commit guidelines

| Prefix | When |
| --- | --- |
| ✨ `feat:` | a new feature |
| 🔧 `fix:` | a bug fix |
| ♻️ `refactor:` | a change that does not add a feature or fix a bug |
| 🗑️ `remove:` | code or packages removed |

import assert from "node:assert/strict"
import { mkdir, rm, symlink, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { describe, it } from "node:test"
import { loadProject } from "../src/load.ts"
import { listen, project } from "./fixture.ts"

describe("server", () => {
  it("serves the built-in page when / is missing", async () => {
    const app = await project("mehrashop")
    const running = await listen(app.dir)
    try {
      const res = await fetch(`${running.url}/`)
      const body = await res.text()
      assert.equal(res.status, 200)
      assert.equal(res.headers.get("content-type"), "text/html; charset=utf-8")
      assert.match(body, /It works/)
      assert.match(body, /mehrashop/)
      assert.match(body, /https:\/\/uoxone\.github\.io\/jango\//)
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("uses a route function and ignores the query string", async () => {
    const app = await project()
    await writeFile(
      join(app.dir, "url.ts"),
      `const url = {\n  "/": () => "Hello",\n}\n\nexport default url\n`,
    )
    const running = await listen(app.dir)
    try {
      const home = await fetch(`${running.url}/?x=1`)
      const missing = await fetch(`${running.url}/missing`)
      assert.equal(await home.text(), "Hello")
      assert.equal(missing.status, 404)
      assert.equal(await missing.text(), "Not Found")
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("serves an html file from templates", async () => {
    const app = await project()
    await writeFile(join(app.dir, "templates", "page.html"), "<p>from file</p>\n")
    await writeFile(
      join(app.dir, "url.ts"),
      `const url = {\n  "/": "page.html",\n}\n\nexport default url\n`,
    )
    const running = await listen(app.dir)
    try {
      const res = await fetch(`${running.url}/?x=1`)
      assert.equal(res.status, 200)
      assert.equal(await res.text(), "<p>from file</p>\n")
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("returns 500 when a handler throws, including async handlers", async () => {
    const app = await project()
    await writeFile(
      join(app.dir, "url.ts"),
      `const url = {
  "/": () => {
    throw new Error("secret")
  },
  "/later": async () => "later",
}

export default url
`,
    )
    const running = await listen(app.dir)
    try {
      const failed = await fetch(`${running.url}/`)
      const later = await fetch(`${running.url}/later`)
      assert.equal(failed.status, 500)
      assert.equal(await failed.text(), "Internal Server Error")
      assert.equal(await later.text(), "later")
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("rejects a project that is not a Jango project", async () => {
    const app = await project()
    await assert.rejects(loadProject(join(app.dir, "missing")), /Not a Jango project/)
    await app.remove()
  })

  it("does not load an html file outside templates", async () => {
    const app = await project()
    await writeFile(join(app.dir, "page.html"), "<p>root</p>\n")
    await writeFile(
      join(app.dir, "url.ts"),
      `const url = {\n  "/": "page.html",\n}\n\nexport default url\n`,
    )
    await assert.rejects(loadProject(app.dir), /Missing template: page.html/)
    await app.remove()
  })

  it("serves files from static", async () => {
    const app = await project()
    const png = Buffer.from([0x89, 0x50, 0x4e, 0x47])
    await mkdir(join(app.dir, "static", "css"))
    await writeFile(join(app.dir, "static", "css", "app.css"), "body{}\n")
    await writeFile(join(app.dir, "static", "icon.png"), png)
    await writeFile(join(app.dir, "static", "data.bin"), "xyz")
    const running = await listen(app.dir)
    try {
      const css = await fetch(`${running.url}/static/css/app.css?v=1`)
      const image = await fetch(`${running.url}/static/icon.png`)
      const bin = await fetch(`${running.url}/static/data.bin`)
      const missing = await fetch(`${running.url}/static/missing.css`)
      const folder = await fetch(`${running.url}/static/css/`)
      assert.equal(css.status, 200)
      assert.equal(css.headers.get("content-type"), "text/css; charset=utf-8")
      assert.equal(await css.text(), "body{}\n")
      assert.equal(image.headers.get("content-type"), "image/png")
      assert.deepEqual(Buffer.from(await image.arrayBuffer()), png)
      assert.equal(bin.headers.get("content-type"), "application/octet-stream")
      assert.equal(missing.status, 404)
      assert.equal(await missing.text(), "Not Found")
      assert.equal(folder.status, 404)
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("does not serve a file outside static", async () => {
    const app = await project()
    await writeFile(join(app.dir, "secret.txt"), "secret")
    await symlink("../secret.txt", join(app.dir, "static", "secret.txt"))
    const running = await listen(app.dir)
    try {
      const link = await fetch(`${running.url}/static/secret.txt`)
      assert.equal(link.status, 404)
      assert.equal(await link.text(), "Not Found")
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("lets a route replace a static file", async () => {
    const app = await project()
    await writeFile(join(app.dir, "static", "app.css"), "body{}")
    await writeFile(
      join(app.dir, "url.ts"),
      `const url = {\n  "/static/app.css": () => "from route",\n}\n\nexport default url\n`,
    )
    const running = await listen(app.dir)
    try {
      const res = await fetch(`${running.url}/static/app.css`)
      assert.equal(await res.text(), "from route")
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("starts when static is missing and rejects a static file", async () => {
    const app = await project()
    await rm(join(app.dir, "static"), { recursive: true })
    const open = await loadProject(app.dir)
    assert.equal(open.files.size, 0)
    await writeFile(join(app.dir, "static"), "nope")
    await assert.rejects(loadProject(app.dir), /staticRoot must be a directory/)
    await app.remove()
  })

  it("serves staticRoot at staticUrl", async () => {
    const app = await project()
    await mkdir(join(app.dir, "assets"))
    await writeFile(join(app.dir, "assets", "app.css"), "body{}")
    await writeFile(join(app.dir, "settings.ts"), settings("assets", "/media/"), "utf8")
    const running = await listen(app.dir)
    try {
      const res = await fetch(`${running.url}/media/app.css`)
      const old = await fetch(`${running.url}/static/app.css`)
      assert.equal(res.status, 200)
      assert.equal(res.headers.get("content-type"), "text/css; charset=utf-8")
      assert.equal(await res.text(), "body{}")
      assert.equal(old.status, 404)
    } finally {
      await running.close()
      await app.remove()
    }
  })

  it("rejects a staticRoot or staticUrl outside the project", async () => {
    await expectStatic("../secret", "/static/", /Invalid staticRoot/)
    await expectStatic("static", "static/", /Invalid staticUrl/)
    await expectStatic("static", "/../", /Invalid staticUrl/)
  })

  it("rejects routes outside templates", async () => {
    const app = await project()
    await writeFile(
      join(app.dir, "url.ts"),
      `const url = {\n  "/": "../secret.html",\n}\n\nexport default url\n`,
    )
    await assert.rejects(loadProject(app.dir), /Invalid template/)
    await app.remove()
  })
})

async function expectStatic(staticRoot: string, staticUrl: string, message: RegExp) {
  const app = await project()
  try {
    await writeFile(join(app.dir, "settings.ts"), settings(staticRoot, staticUrl), "utf8")
    await assert.rejects(loadProject(app.dir), message)
  } finally {
    await app.remove()
  }
}

function settings(staticRoot: string, staticUrl: string) {
  return `const settings = {
  host: "127.0.0.1",
  port: 8000,
  staticRoot: ${JSON.stringify(staticRoot)},
  staticUrl: ${JSON.stringify(staticUrl)},
}

export default settings
`
}

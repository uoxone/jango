import assert from "node:assert/strict"
import { writeFile } from "node:fs/promises"
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

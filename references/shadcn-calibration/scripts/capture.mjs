import { chromium } from "@playwright/test"
import { spawn, spawnSync } from "node:child_process"
import { mkdir, writeFile } from "node:fs/promises"
import net from "node:net"
import path from "node:path"
import { fileURLToPath } from "node:url"

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(__dirname, "..")
const outDir = path.join(root, "out")
const host = "127.0.0.1"
let port = Number(process.env.SHADCN_REFERENCE_PORT ?? 0)
let baseUrl
let stoppingVite = false

const viewport = {
  width: Number(process.env.SHADCN_REFERENCE_WIDTH ?? 1180),
  height: Number(process.env.SHADCN_REFERENCE_HEIGHT ?? 780),
}
const deviceScaleFactor = Number(process.env.SHADCN_REFERENCE_DSF ?? 1)
const uiScale = Number(process.env.SHADCN_REFERENCE_UI_SCALE ?? 1)

const captures = [
  {
    slug: "shadcn-calibration",
    path: "/",
    description: "Local component/surface calibration reference",
  },
  {
    slug: "shadcn-dashboard-01",
    path: "/?view=dashboard-01",
    description: "Dashboard-01-style reference",
  },
  {
    slug: "shadcn-settings-01",
    path: "/?view=settings-01",
    description: "Settings/form reference",
  },
]

async function main() {
  await mkdir(outDir, { recursive: true })
  port = port || await pickFreePort()
  baseUrl = `http://${host}:${port}`
  const server = startVite()
  try {
    await waitForServer(server)
    await captureAll()
  } finally {
    stoppingVite = true
    server.kill("SIGTERM")
  }
}

function startVite() {
  const child = spawn(
    "npm",
    ["exec", "vite", "--", "--host", host, "--port", String(port), "--strictPort"],
    {
      cwd: root,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        BROWSER: "none",
      },
    },
  )
  child.stdout.on("data", (chunk) => process.stdout.write(chunk))
  child.stderr.on("data", (chunk) => process.stderr.write(chunk))
  child.on("exit", (code, signal) => {
    if (stoppingVite && (signal === "SIGTERM" || code === 143 || code === 0)) {
      return
    }
    if (code !== null && code !== 0) {
      console.error(`vite exited with code ${code}`)
    }
    if (signal && signal !== "SIGTERM") {
      console.error(`vite exited from signal ${signal}`)
    }
  })
  return child
}

async function waitForServer(server) {
  const deadline = Date.now() + 15_000
  let lastError
  let exited = false
  server.once("exit", () => {
    exited = true
  })
  while (Date.now() < deadline) {
    if (exited) {
      throw new Error("Vite exited before the capture server became ready")
    }
    try {
      const response = await fetch(baseUrl)
      if (response.ok) {
        return
      }
      lastError = new Error(`HTTP ${response.status}`)
    } catch (error) {
      lastError = error
    }
    await sleep(150)
  }
  throw new Error(`Timed out waiting for ${baseUrl}: ${lastError?.message ?? "unknown error"}`)
}

async function captureAll() {
  const executablePath = process.env.CHROMIUM_PATH || findChromium()
  const browser = await chromium.launch({
    executablePath,
    headless: true,
    args: [
      "--force-device-scale-factor=1",
      "--high-dpi-support=1",
      "--no-sandbox",
    ],
  })
  try {
    const context = await browser.newContext({
      viewport,
      deviceScaleFactor,
      colorScheme: "dark",
      reducedMotion: "reduce",
    })

    for (const item of captures) {
      const page = await context.newPage()
      const url = new URL(item.path, baseUrl)
      url.searchParams.set("uiScale", String(uiScale))
      await page.goto(url.toString(), { waitUntil: "networkidle" })
      await page.emulateMedia({ colorScheme: "dark", reducedMotion: "reduce" })
      const overflowFindings = await referenceOverflowFindings(page)
      if (overflowFindings.length > 0) {
        throw new Error(
          [
            `${item.slug} has reference overflow findings:`,
            ...overflowFindings.map((finding) =>
              `  ${finding.boundary} -> ${finding.child} overflow L=${finding.left} R=${finding.right} T=${finding.top} B=${finding.bottom}`,
            ),
          ].join("\n"),
        )
      }

      const pngPath = path.join(outDir, `${item.slug}.png`)
      const metadataPath = path.join(outDir, `${item.slug}.json`)
      await page.screenshot({ path: pngPath, fullPage: false })
      const metadata = await page.evaluate(() => ({
        devicePixelRatio: window.devicePixelRatio,
        innerWidth: window.innerWidth,
        innerHeight: window.innerHeight,
        outerWidth: window.outerWidth,
        outerHeight: window.outerHeight,
        visualViewportScale: window.visualViewport?.scale ?? null,
        rootFontSize: getComputedStyle(document.documentElement).fontSize,
        bodyFontSize: getComputedStyle(document.body).fontSize,
      }))
      await writeFile(
        metadataPath,
        JSON.stringify(
          {
            slug: item.slug,
            description: item.description,
            url: url.toString(),
            viewport,
            requestedDeviceScaleFactor: deviceScaleFactor,
            requestedUiScale: uiScale,
            capturedAt: new Date().toISOString(),
            ...metadata,
          },
          null,
          2,
        ) + "\n",
      )
      console.log(`wrote ${pngPath}`)
      console.log(`wrote ${metadataPath}`)
      await page.close()
    }
  } finally {
    await browser.close()
  }
}

async function referenceOverflowFindings(page) {
  return page.evaluate(() => {
    const tolerance = 0.5
    const findings = []
    const visible = (rect) => rect.width > 0.5 && rect.height > 0.5
    const describe = (el) => {
      const tag = el.tagName.toLowerCase()
      const text = (el.textContent ?? "").trim().replace(/\s+/g, " ").slice(0, 40)
      return text ? `${tag} "${text}"` : tag
    }

    for (const boundary of document.querySelectorAll("[data-calibration-boundary]")) {
      const boundaryRect = boundary.getBoundingClientRect()
      if (!visible(boundaryRect)) {
        continue
      }
      for (const child of boundary.querySelectorAll("*")) {
        if (child.closest("[data-calibration-boundary]") !== boundary) {
          continue
        }
        const childRect = child.getBoundingClientRect()
        if (!visible(childRect)) {
          continue
        }
        const left = Math.max(0, boundaryRect.left - childRect.left)
        const right = Math.max(0, childRect.right - boundaryRect.right)
        const top = Math.max(0, boundaryRect.top - childRect.top)
        const bottom = Math.max(0, childRect.bottom - boundaryRect.bottom)
        if (
          left > tolerance ||
          right > tolerance ||
          top > tolerance ||
          bottom > tolerance
        ) {
          findings.push({
            boundary: describe(boundary),
            child: describe(child),
            left: Math.round(left),
            right: Math.round(right),
            top: Math.round(top),
            bottom: Math.round(bottom),
          })
        }
      }
    }
    return findings
  })
}

function findChromium() {
  for (const name of ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable"]) {
    const result = spawnSync("which", [name], { encoding: "utf8" })
    if (result.status === 0) {
      return result.stdout.trim()
    }
  }
  return undefined
}

function pickFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer()
    server.once("error", reject)
    server.listen(0, host, () => {
      const address = server.address()
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("Could not allocate a local port")))
        return
      }
      const picked = address.port
      server.close(() => resolve(picked))
    })
  })
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})

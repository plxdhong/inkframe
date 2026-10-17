package com.inkframe.app.provision

import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Test
import java.net.HttpURLConnection
import java.net.URI

/**
 * Runs the real Kotlin client against a running daemon's provisioning
 * simulator (config/dev.toml). Skipped unless both are set, e.g.
 *   INKFRAME_SIM=host.docker.internal:8090 INKFRAME_API=http://host.docker.internal:8080
 * The daemon must be listening on an address the test can reach.
 */
class SimEndToEndTest {
    private val sim = System.getenv("INKFRAME_SIM").orEmpty()
    private val api = System.getenv("INKFRAME_API").orEmpty()

    private fun http(method: String, path: String): String {
        val c = URI("$api/api/v1$path").toURL().openConnection() as HttpURLConnection
        c.requestMethod = method
        c.connectTimeout = 3_000
        c.readTimeout = 10_000
        return c.inputStream.bufferedReader().readText()
    }

    @Test
    fun provisionThroughSimulator() = runBlocking {
        assumeTrue("set INKFRAME_SIM and INKFRAME_API to run", sim.isNotEmpty() && api.isNotEmpty())
        http("POST", "/provision/start")
        // The code normally comes off the panel; the debug endpoint stands in for eyes.
        val code = Regex(""""code":"(\w+)"""").find(http("GET", "/debug/provision-code"))?.groupValues?.get(1).orEmpty()
        val t = SimTransport.connect(sim.substringBeforeLast(':'), sim.substringAfterLast(':').toInt())
        t.use {
            val client = ProvisioningClient(t)
            val info = client.info()
            assertTrue(info.name.startsWith("inkframe-"))

            // A wrong code is rejected and the session can retry.
            runCatching { client.authenticate("0000-0000-0000") }.let { assertTrue(it.exceptionOrNull() is WrongCodeException) }
            client.authenticate(code)

            val nets = client.scan()
            assertTrue(nets.any { it.ssid == "Home" })

            val bad = client.connect("Home", "wrong-password", false).toList()
            assertEquals(FailReason.AUTH_FAILED, (bad.last() as WifiState.Failed).reason)

            val good = client.connect("Home", "inkframe123", false).toList()
            assertTrue(good.first() is WifiState.Connecting)
            val done = good.last() as WifiState.Connected
            assertEquals("Home", done.ssid)
            assertTrue(done.host.endsWith(".local"))

            assertEquals(done, client.status())
            client.done()
        }
        // The device leaves provisioning mode on "done".
        var active = true
        repeat(20) {
            if (active) {
                active = http("GET", "/provision").contains(""""active":true""")
                if (active) Thread.sleep(500)
            }
        }
        assertTrue("provisioning should have ended", !active)
    }
}

package com.inkframe.app

import com.inkframe.app.data.Device
import com.inkframe.app.data.pickHost
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.net.InetAddress

class DiscoveryTest {
    private fun ip(s: String) = InetAddress.getByName(s)

    @Test
    fun prefersIpv4OverLinkLocalV6() {
        assertEquals("192.0.2.10", pickHost(listOf(ip("fe80::1"), ip("192.0.2.10"))))
    }

    @Test
    fun linkLocalOnlyIsUnusable() {
        assertNull(pickHost(listOf(ip("fe80::1"))))
    }

    @Test
    fun globalV6IsBracketedInUrls() {
        val host = pickHost(listOf(ip("fe80::1"), ip("2001:db8::10")))
        assertEquals("http://[2001:db8:0:0:0:0:0:10]:8080", Device("3fa2", "inkframe-3fa2", host ?: "", 8080).baseUrl)
    }
}

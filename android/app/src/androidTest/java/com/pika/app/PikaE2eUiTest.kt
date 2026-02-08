package com.pika.app

import android.util.Log
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import com.pika.app.ui.TestTags
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assume
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class PikaE2eUiTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    @Test
    fun e2e_deployedRustBot_pingPong() {
        val args = InstrumentationRegistry.getArguments()
        Assume.assumeTrue(
            "Set -Pandroid.testInstrumentationRunnerArguments.pika_e2e=1 to enable public-relay E2E UI test.",
            args.getString("pika_e2e") == "1",
        )

        val ctx = InstrumentationRegistry.getInstrumentation().targetContext
        val testNsec = args.getString("pika_nsec") ?: args.getString("pika_test_nsec") ?: ""
        val botNpub = args.getString("pika_peer_npub") ?: ""

        // Public-relay E2E should be explicit. Defaults hide misconfiguration and cause flaky hangs.
        check(botNpub.isNotBlank()) { "Missing instrumentation arg: pika_peer_npub" }
        check(testNsec.isNotBlank()) { "Missing instrumentation arg: pika_nsec" }

        // Ensure we start from a known auth state.
        runOnMain { AppManager.getInstance(ctx).logout() }
        compose.waitForIdle()

        // Wait for login UI to be ready; the Activity may take a moment to transition.
        compose.waitUntil(30_000) {
            runCatching {
                compose.onAllNodesWithTag(TestTags.LOGIN_CREATE_ACCOUNT).fetchSemanticsNodes().isNotEmpty()
            }.getOrDefault(false) ||
                runCatching { compose.onAllNodesWithText("Chats").fetchSemanticsNodes().isNotEmpty() }.getOrDefault(false)
        }

        // Prefer logging into a stable allowlisted identity when provided.
        val alreadyInChats =
            runCatching { compose.onNodeWithText("Chats").assertIsDisplayed() }.isSuccess

        if (!alreadyInChats && testNsec.isNotBlank()) {
            compose.onNodeWithTag(TestTags.LOGIN_NSEC).performTextInput(testNsec)
            compose.onNodeWithTag(TestTags.LOGIN_LOGIN).performClick()
        } else if (!alreadyInChats) {
            // Create account (may not be able to talk to the deployed bot if it enforces allowlists).
            compose.onNodeWithTag(TestTags.LOGIN_CREATE_ACCOUNT).performClick()
        }

        compose.waitUntil(30_000) {
            runCatching { compose.onNodeWithText("Chats").assertIsDisplayed() }.isSuccess
        }

        Log.d("PikaE2eUiTest", "botNpub=$botNpub")

        // New chat with deployed bot.
        compose.onNodeWithContentDescription("New Chat").performClick()
        dumpState("after New Chat click", ctx)

        // Don't assert on the TopAppBar title. Material3 can merge semantics such that the
        // matching text node exists but is not "displayed" per test semantics. The peer input
        // field is a better screen-ready signal.
        compose.waitUntil(30_000) {
            runCatching {
                compose.onAllNodesWithTag(TestTags.NEWCHAT_PEER_NPUB).fetchSemanticsNodes().isNotEmpty()
            }.getOrDefault(false)
        }
        compose.onNodeWithTag(TestTags.NEWCHAT_PEER_NPUB).performTextInput(botNpub)
        compose.waitForIdle()

        // Sanity check: ensure the full peer id made it into the field.
        val actualPeer =
            runCatching {
                compose.onNodeWithTag(TestTags.NEWCHAT_PEER_NPUB)
                    .fetchSemanticsNode()
                    .config
                    .getOrNull(SemanticsProperties.EditableText)
                    ?.text
            }.getOrNull()
        Log.d("PikaE2eUiTest", "peerField=${actualPeer ?: "<unknown>"}")

        compose.onNodeWithTag(TestTags.NEWCHAT_START).assertIsEnabled()
        compose.onNodeWithTag(TestTags.NEWCHAT_START).performClick()

        dumpState("after Start chat click", ctx)

        // Wait for chat composer.
        compose.waitUntil(120_000) {
            runCatching {
                compose.onAllNodesWithTag(TestTags.CHAT_MESSAGE_INPUT).fetchSemanticsNodes().isNotEmpty()
            }.getOrDefault(false)
        }
        check(compose.onAllNodesWithTag(TestTags.CHAT_MESSAGE_INPUT).fetchSemanticsNodes().isNotEmpty())

        // Send deterministic probe.
        val nonce = java.util.UUID.randomUUID().toString().replace("-", "").lowercase()
        val probe = "ping:$nonce"
        val expect = "pong:$nonce"
        compose.onNodeWithTag(TestTags.CHAT_MESSAGE_INPUT).performTextInput(probe)
        compose.waitForIdle()
        compose.onNodeWithTag(TestTags.CHAT_SEND).performClick()

        dumpState("after probe", ctx)

        // Wait for deterministic ack from the bot. Prefer state inspection (Rust-owned) to avoid keyboard overlap flakes.
        compose.waitUntil(180_000) {
            val hasPong =
                runOnMain {
                    AppManager.getInstance(ctx).state.currentChat?.messages?.any {
                        it.content.trim() == expect
                    }
                        ?: false
                }
            if (!hasPong) return@waitUntil false
            runCatching { compose.onAllNodesWithText(expect).fetchSemanticsNodes().isNotEmpty() }
                .getOrDefault(false)
        }
        check(compose.onAllNodesWithText(expect).fetchSemanticsNodes().isNotEmpty())
    }

    private fun dumpState(phase: String, ctx: android.content.Context) {
        runCatching {
            val st = AppManager.getInstance(ctx).state
            val msgCount = st.currentChat?.messages?.size ?: 0
            val lastMsg = st.currentChat?.messages?.lastOrNull()?.content
            Log.d(
                "PikaE2eUiTest",
                "phase=$phase rev=${st.rev} default=${st.router.defaultScreen} stack=${st.router.screenStack} chats=${st.chatList.size} current=${st.currentChat?.chatId} msgCount=$msgCount lastMsg=${lastMsg ?: ""}",
            )
        }
    }

    private fun <T> runOnMain(block: () -> T): T {
        val ref = AtomicReference<T>()
        InstrumentationRegistry.getInstrumentation().runOnMainSync { ref.set(block()) }
        @Suppress("UNCHECKED_CAST")
        return ref.get() as T
    }
}

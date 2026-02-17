package com.pika.app

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.pika.app.rust.AppAction
import com.pika.app.rust.AppReconciler
import com.pika.app.rust.AppState
import com.pika.app.rust.AppUpdate
import com.pika.app.rust.AuthMode
import com.pika.app.rust.AuthState
import com.pika.app.rust.ExternalSignerBridge
import com.pika.app.rust.ExternalSignerErrorKind
import com.pika.app.rust.ExternalSignerResult
import com.pika.app.rust.FfiApp
import com.pika.app.rust.MyProfileState
import java.io.File
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import org.json.JSONObject

class AppManager private constructor(context: Context) : AppReconciler {
    private val appContext = context.applicationContext
    private val mainHandler = Handler(Looper.getMainLooper())
    private val secureStore = SecureAuthStore(appContext)
    private val amberClient = AmberSignerClient(appContext)
    private val activeExternalDescriptor = AtomicReference<AmberDescriptor?>(null)
    private val signerRequestLock = Any()
    private val audioFocus = AndroidAudioFocusManager(appContext)
    private val rust: FfiApp
    private var lastRevApplied: ULong = 0UL
    private val listening = AtomicBoolean(false)

    var amberLoginInProgress by mutableStateOf(false)
        private set

    var state: AppState by mutableStateOf(
        AppState(
            rev = 0UL,
            router = com.pika.app.rust.Router(
                defaultScreen = com.pika.app.rust.Screen.Login,
                screenStack = emptyList(),
            ),
            auth = com.pika.app.rust.AuthState.LoggedOut,
            myProfile = MyProfileState(name = "", about = "", pictureUrl = null),
            busy = com.pika.app.rust.BusyState(
                creatingAccount = false,
                loggingIn = false,
                creatingChat = false,
                fetchingFollowList = false,
            ),
            chatList = emptyList(),
            currentChat = null,
            followList = emptyList(),
            peerProfile = null,
            activeCall = null,
            toast = null,
        ),
    )
        private set

    init {
        // Ensure call config is present before Rust bootstraps. If the file already exists (e.g.
        // created by tooling), only fill missing keys to avoid clobbering overrides.
        ensureDefaultConfig(appContext)

        val dataDir = appContext.filesDir.absolutePath
        rust = FfiApp(dataDir)
        if (BuildConfig.ENABLE_AMBER_SIGNER) {
            rust.setExternalSignerBridge(AmberRustBridge())
        }
        val initial = rust.state()
        state = initial
        audioFocus.syncForCall(initial.activeCall)
        lastRevApplied = initial.rev
        startListening()
        restoreSessionFromSecureStore()
    }

    private fun ensureDefaultConfig(context: Context) {
        val filesDir = context.filesDir
        val path = File(filesDir, "pika_config.json")
        val defaultMoqUrl = "https://us-east.moq.logos.surf/anon"
        val defaultBroadcastPrefix = "pika/calls"

        val obj =
            runCatching {
                if (path.exists()) {
                    JSONObject(path.readText())
                } else {
                    JSONObject()
                }
            }.getOrElse { JSONObject() }

        fun isMissingOrBlank(key: String): Boolean {
            if (!obj.has(key)) return true
            val v = obj.optString(key, "").trim()
            return v.isEmpty()
        }

        if (isMissingOrBlank("call_moq_url")) {
            obj.put("call_moq_url", defaultMoqUrl)
        }
        if (isMissingOrBlank("call_broadcast_prefix")) {
            obj.put("call_broadcast_prefix", defaultBroadcastPrefix)
        }
        // Keep Rust-side signer gating in sync with Android build-time flag.
        // If callers provided an explicit value, respect it.
        if (!obj.has("enable_external_signer")) {
            obj.put("enable_external_signer", BuildConfig.ENABLE_AMBER_SIGNER)
        }

        runCatching {
            val tmp = File(filesDir, "pika_config.json.tmp")
            tmp.writeText(obj.toString())
            if (!tmp.renameTo(path)) {
                // Fallback for devices that don't allow rename across filesystems (shouldn't happen in app filesDir).
                path.writeText(obj.toString())
                tmp.delete()
            }
        }
    }

    private fun startListening() {
        if (!listening.compareAndSet(false, true)) return
        rust.listenForUpdates(this)
    }

    fun dispatch(action: AppAction) {
        rust.dispatch(action)
    }

    fun loginWithNsec(nsec: String) {
        val trimmed = nsec.trim()
        if (trimmed.isNotBlank()) {
            secureStore.saveLocalNsec(trimmed)
        }
        activeExternalDescriptor.set(null)
        rust.dispatch(AppAction.Login(trimmed))
    }

    fun loginWithAmber() {
        if (!BuildConfig.ENABLE_AMBER_SIGNER) {
            showToast("Amber signer is disabled")
            return
        }
        if (amberLoginInProgress) return
        amberLoginInProgress = true

        val currentUserHint =
            activeExternalDescriptor.get()?.currentUser
                ?: secureStore.load()?.takeIf { it.mode == StoredAuthMode.EXTERNAL_SIGNER }?.currentUser

        thread(name = "amber-login", start = true) {
            val result = withSignerRequestLock { amberClient.requestPublicKey(currentUserHint) }
            mainHandler.post {
                amberLoginInProgress = false
                if (!result.ok) {
                    showToast(publicKeyErrorMessage(result))
                    return@post
                }

                val pubkey = result.pubkey?.trim().orEmpty()
                val signerPackage = result.signerPackage?.trim().orEmpty()
                if (pubkey.isBlank() || signerPackage.isBlank()) {
                    showToast("Amber returned an invalid account")
                    return@post
                }

                val currentUser =
                    result.currentUser
                        ?.trim()
                        ?.takeIf { it.isNotEmpty() }
                        ?: pubkey
                activeExternalDescriptor.set(
                    AmberDescriptor(
                        pubkey = pubkey,
                        signerPackage = signerPackage,
                        currentUser = currentUser,
                    ),
                )
                rust.dispatch(
                    AppAction.LoginWithExternalSigner(
                        pubkey = pubkey,
                        signerPackage = signerPackage,
                    ),
                )
            }
        }
    }

    fun logout() {
        secureStore.clear()
        activeExternalDescriptor.set(null)
        rust.dispatch(AppAction.Logout)
    }

    fun onForeground() {
        // Foreground is a lifecycle signal; Rust owns state changes and side effects.
        rust.dispatch(AppAction.Foregrounded)
    }

    override fun reconcile(update: AppUpdate) {
        mainHandler.post {
            val updateRev = update.rev()

            // Side-effect updates must not be lost: `AccountCreated` carries an `nsec` that isn't in
            // AppState snapshots (by design). Store it even if the update is stale w.r.t. rev.
            if (update is AppUpdate.AccountCreated) {
                val existing = secureStore.load()?.nsec.orEmpty()
                if (existing.isBlank() && update.nsec.isNotBlank()) {
                    secureStore.saveLocalNsec(update.nsec)
                }
            }

            // The stream is full-state snapshots; drop anything stale.
            if (updateRev <= lastRevApplied) return@post

            lastRevApplied = updateRev
            when (update) {
                is AppUpdate.FullState -> state = update.v1
                is AppUpdate.AccountCreated -> {
                    // Required by spec-v2: native stores nsec; Rust never persists it.
                    if (update.nsec.isNotBlank()) {
                        secureStore.saveLocalNsec(update.nsec)
                    }
                    state = state.copy(rev = updateRev)
                }
            }
            syncSecureStoreWithAuthState()
            audioFocus.syncForCall(state.activeCall)
        }
    }

    private fun AppUpdate.rev(): ULong =
        when (this) {
            is AppUpdate.FullState -> this.v1.rev
            is AppUpdate.AccountCreated -> this.rev
        }

    private fun restoreSessionFromSecureStore() {
        val stored = secureStore.load() ?: return
        when (stored.mode) {
            StoredAuthMode.LOCAL_NSEC -> {
                val nsec = stored.nsec?.trim().orEmpty()
                if (nsec.isNotEmpty()) {
                    rust.dispatch(AppAction.RestoreSession(nsec))
                }
            }
            StoredAuthMode.EXTERNAL_SIGNER -> {
                if (!BuildConfig.ENABLE_AMBER_SIGNER) return
                val pubkey = stored.pubkey?.trim().orEmpty()
                val signerPackage = stored.signerPackage?.trim().orEmpty()
                if (pubkey.isBlank() || signerPackage.isBlank()) return
                val currentUser = stored.currentUser?.trim().takeUnless { it.isNullOrEmpty() } ?: pubkey
                activeExternalDescriptor.set(
                    AmberDescriptor(
                        pubkey = pubkey,
                        signerPackage = signerPackage,
                        currentUser = currentUser,
                    ),
                )
                rust.dispatch(
                    AppAction.RestoreSessionExternalSigner(
                        pubkey = pubkey,
                        signerPackage = signerPackage,
                    ),
                )
            }
        }
    }

    private fun syncSecureStoreWithAuthState() {
        when (val auth = state.auth) {
            is AuthState.LoggedOut -> Unit
            is AuthState.LoggedIn -> {
                when (val mode = auth.mode) {
                    is AuthMode.LocalNsec -> {
                        activeExternalDescriptor.set(null)
                        if (secureStore.load()?.mode == StoredAuthMode.EXTERNAL_SIGNER) {
                            secureStore.clear()
                        }
                    }
                    is AuthMode.ExternalSigner -> {
                        val currentUser =
                            activeExternalDescriptor
                                .get()
                                ?.currentUser
                                ?.takeIf { it.isNotBlank() }
                                ?: auth.pubkey
                        activeExternalDescriptor.set(
                            AmberDescriptor(
                                pubkey = mode.pubkey,
                                signerPackage = mode.signerPackage,
                                currentUser = currentUser,
                            ),
                        )
                        secureStore.saveExternalSigner(
                            pubkey = mode.pubkey,
                            signerPackage = mode.signerPackage,
                            currentUser = currentUser,
                        )
                    }
                }
            }
        }
    }

    private fun publicKeyErrorMessage(result: AmberPublicKeyResult): String =
        when (result.kind) {
            AmberErrorKind.REJECTED -> "Amber request rejected"
            AmberErrorKind.CANCELED -> "Amber request canceled"
            AmberErrorKind.TIMEOUT -> "Amber request timed out"
            AmberErrorKind.SIGNER_UNAVAILABLE -> "Amber signer unavailable"
            AmberErrorKind.PACKAGE_MISMATCH -> "Amber signer package mismatch"
            AmberErrorKind.INVALID_RESPONSE ->
                result.message?.takeIf { it.isNotBlank() } ?: "Amber returned an invalid response"
            AmberErrorKind.OTHER, null -> result.message?.takeIf { it.isNotBlank() } ?: "Amber login failed"
        }

    private fun showToast(message: String) {
        if (message.isBlank()) return
        if (Looper.myLooper() == Looper.getMainLooper()) {
            Toast.makeText(appContext, message, Toast.LENGTH_SHORT).show()
            return
        }
        mainHandler.post {
            Toast.makeText(appContext, message, Toast.LENGTH_SHORT).show()
        }
    }

    private inline fun <T> withSignerRequestLock(block: () -> T): T = synchronized(signerRequestLock) { block() }

    private inner class AmberRustBridge : ExternalSignerBridge {
        override fun signEvent(unsignedEventJson: String): ExternalSignerResult =
            withSignerRequestLock {
                withActiveDescriptor { descriptor ->
                    amberClient.signEvent(descriptor, unsignedEventJson).toExternalSignerResult()
                }
            }

        override fun nip44Encrypt(peerPubkey: String, content: String): ExternalSignerResult =
            withSignerRequestLock {
                withActiveDescriptor { descriptor ->
                    amberClient.nip44Encrypt(descriptor, peerPubkey, content).toExternalSignerResult()
                }
            }

        override fun nip44Decrypt(peerPubkey: String, payload: String): ExternalSignerResult =
            withSignerRequestLock {
                withActiveDescriptor { descriptor ->
                    amberClient.nip44Decrypt(descriptor, peerPubkey, payload).toExternalSignerResult()
                }
            }

        override fun nip04Encrypt(peerPubkey: String, content: String): ExternalSignerResult =
            withSignerRequestLock {
                withActiveDescriptor { descriptor ->
                    amberClient.nip04Encrypt(descriptor, peerPubkey, content).toExternalSignerResult()
                }
            }

        override fun nip04Decrypt(peerPubkey: String, payload: String): ExternalSignerResult =
            withSignerRequestLock {
                withActiveDescriptor { descriptor ->
                    amberClient.nip04Decrypt(descriptor, peerPubkey, payload).toExternalSignerResult()
                }
            }

        private fun withActiveDescriptor(
            f: (AmberDescriptor) -> ExternalSignerResult,
        ): ExternalSignerResult {
            val descriptor = activeExternalDescriptor.get()
            if (descriptor == null) {
                return ExternalSignerResult(
                    ok = false,
                    value = null,
                    errorKind = ExternalSignerErrorKind.SIGNER_UNAVAILABLE,
                    errorMessage = "signer unavailable: no active signer descriptor",
                )
            }
            return f(descriptor)
        }

        private fun AmberResult.toExternalSignerResult(): ExternalSignerResult =
            ExternalSignerResult(
                ok = ok,
                value = value,
                errorKind = kind?.toExternalSignerErrorKind(),
                errorMessage = message,
            )

        private fun AmberErrorKind.toExternalSignerErrorKind(): ExternalSignerErrorKind =
            when (this) {
                AmberErrorKind.REJECTED -> ExternalSignerErrorKind.REJECTED
                AmberErrorKind.CANCELED -> ExternalSignerErrorKind.CANCELED
                AmberErrorKind.TIMEOUT -> ExternalSignerErrorKind.TIMEOUT
                AmberErrorKind.SIGNER_UNAVAILABLE -> ExternalSignerErrorKind.SIGNER_UNAVAILABLE
                AmberErrorKind.PACKAGE_MISMATCH -> ExternalSignerErrorKind.PACKAGE_MISMATCH
                AmberErrorKind.INVALID_RESPONSE -> ExternalSignerErrorKind.INVALID_RESPONSE
                AmberErrorKind.OTHER -> ExternalSignerErrorKind.OTHER
            }
    }

    companion object {
        @Volatile
        private var instance: AppManager? = null

        fun getInstance(context: Context): AppManager =
            instance ?: synchronized(this) {
                instance ?: AppManager(context.applicationContext).also { instance = it }
            }
    }
}

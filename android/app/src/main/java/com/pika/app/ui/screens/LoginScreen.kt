package com.pika.app.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Clear
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import com.pika.app.AppManager
import com.pika.app.rust.AppAction
import com.pika.app.ui.TestTags

@Composable
fun LoginScreen(manager: AppManager, padding: PaddingValues) {
    var nsec by remember { mutableStateOf("") }
    var nsecVisible by remember { mutableStateOf(false) }
    var bunkerUri by remember { mutableStateOf("") }
    val busy = manager.state.busy
    val createBusy = busy.creatingAccount
    val loginBusy = busy.loggingIn
    val anyBusy = createBusy || loginBusy

    Column(
        modifier =
            Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Pika")

        Button(
            onClick = {
                manager.dispatch(AppAction.CreateAccount)
            },
            enabled = !anyBusy,
            modifier = Modifier.testTag(TestTags.LOGIN_CREATE_ACCOUNT),
        ) {
            if (createBusy) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Create Account")
            }
        }

        HorizontalDivider(modifier = Modifier.padding(vertical = 10.dp))

        OutlinedTextField(
            value = nsec,
            onValueChange = { nsec = it },
            singleLine = true,
            enabled = !anyBusy,
            label = { Text("nsec") },
            visualTransformation = if (nsecVisible) VisualTransformation.None else PasswordVisualTransformation(),
            keyboardOptions =
                KeyboardOptions(
                    autoCorrect = false,
                    keyboardType = KeyboardType.Password,
                ),
            trailingIcon = {
                if (nsec.isNotEmpty()) {
                    IconButton(onClick = { nsec = "" }) {
                        Icon(Icons.Default.Clear, contentDescription = "Clear")
                    }
                } else {
                    IconButton(onClick = { nsecVisible = !nsecVisible }) {
                        Icon(
                            if (nsecVisible) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                            contentDescription = if (nsecVisible) "Hide" else "Show",
                        )
                    }
                }
            },
            modifier = Modifier.fillMaxWidth().testTag(TestTags.LOGIN_NSEC),
        )

        Button(
            onClick = {
                manager.loginWithNsec(nsec)
            },
            enabled = !anyBusy,
            modifier = Modifier.testTag(TestTags.LOGIN_LOGIN),
        ) {
            if (loginBusy) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Login")
            }
        }

        HorizontalDivider(modifier = Modifier.padding(vertical = 10.dp))

        OutlinedTextField(
            value = bunkerUri,
            onValueChange = { bunkerUri = it },
            singleLine = true,
            enabled = !anyBusy,
            label = { Text("bunker URI") },
            trailingIcon = {
                if (bunkerUri.isNotEmpty()) {
                    IconButton(onClick = { bunkerUri = "" }) {
                        Icon(Icons.Default.Clear, contentDescription = "Clear")
                    }
                }
            },
            modifier = Modifier.fillMaxWidth().testTag(TestTags.LOGIN_BUNKER_URI),
        )

        Button(
            onClick = {
                manager.loginWithBunker(bunkerUri)
            },
            enabled = !anyBusy,
            modifier = Modifier.testTag(TestTags.LOGIN_WITH_BUNKER),
        ) {
            if (loginBusy) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Login with Bunker")
            }
        }

        HorizontalDivider(modifier = Modifier.padding(vertical = 10.dp))
        Button(
            onClick = {
                manager.loginWithNostrConnect()
            },
            enabled = !anyBusy,
            modifier = Modifier.testTag(TestTags.LOGIN_WITH_NOSTR_CONNECT),
        ) {
            if (loginBusy) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Login with Nostr Connect")
            }
        }

        HorizontalDivider(modifier = Modifier.padding(vertical = 10.dp))
        Button(
            onClick = {
                manager.loginWithAmber()
            },
            enabled = !anyBusy,
            modifier = Modifier.testTag(TestTags.LOGIN_WITH_AMBER),
        ) {
            if (loginBusy) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Login with Amber")
            }
        }
    }
}

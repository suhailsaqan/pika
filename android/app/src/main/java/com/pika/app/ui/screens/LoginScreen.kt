package com.pika.app.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import com.pika.app.AppManager
import com.pika.app.rust.AppAction
import com.pika.app.ui.TestTags

@Composable
fun LoginScreen(manager: AppManager, padding: PaddingValues) {
    var nsec by remember { mutableStateOf("") }
    var isLoading by remember { mutableStateOf(false) }

    // Reset loading on error (errors produce toasts).
    LaunchedEffect(manager.state.toast) {
        if (manager.state.toast != null) isLoading = false
    }

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
                isLoading = true
                manager.dispatch(AppAction.CreateAccount)
            },
            enabled = !isLoading,
            modifier = Modifier.testTag(TestTags.LOGIN_CREATE_ACCOUNT),
        ) {
            if (isLoading) {
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
            enabled = !isLoading,
            label = { Text("nsec (mock)") },
            modifier = Modifier.fillMaxWidth().testTag(TestTags.LOGIN_NSEC),
        )

        Button(
            onClick = {
                isLoading = true
                manager.loginWithNsec(nsec)
            },
            enabled = !isLoading,
            modifier = Modifier.testTag(TestTags.LOGIN_LOGIN),
        ) {
            if (isLoading) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                Text("Login")
            }
        }
    }
}

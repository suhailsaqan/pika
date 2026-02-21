package com.pika.app.ui.screens

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.pika.app.AppManager
import com.pika.app.rust.AppAction
import com.pika.app.rust.DeviceInfo
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

@Composable
@OptIn(ExperimentalMaterial3Api::class)
fun DeviceManagementScreen(manager: AppManager, padding: PaddingValues) {
    val devices = manager.state.myDevices
    val pendingDevices = manager.state.pendingDevices
    val autoAddDevices = manager.state.autoAddDevices

    LaunchedEffect(Unit) {
        manager.dispatch(AppAction.FetchMyDevices)
    }

    Scaffold(
        modifier = Modifier.padding(padding),
        topBar = {
            TopAppBar(
                title = { Text("Devices") },
                navigationIcon = {
                    IconButton(
                        onClick = {
                            val stack = manager.state.router.screenStack
                            manager.dispatch(AppAction.UpdateScreenStack(stack.dropLast(1)))
                        },
                    ) {
                        Icon(Icons.Default.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { inner ->
        LazyColumn(
            modifier = Modifier.fillMaxSize().padding(inner),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            // Auto-add toggle
            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            "Auto-add new devices",
                            style = MaterialTheme.typography.bodyLarge,
                        )
                        Text(
                            "Automatically detect and invite new devices to all groups.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Switch(
                        checked = autoAddDevices,
                        onCheckedChange = { manager.dispatch(AppAction.SetAutoAddDevices(it)) },
                    )
                }
            }

            // Pending devices section
            if (pendingDevices.isNotEmpty()) {
                item { HorizontalDivider() }

                item {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            "Pending Devices (${pendingDevices.size})",
                            style = MaterialTheme.typography.titleSmall,
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            Button(
                                onClick = { manager.dispatch(AppAction.AcceptAllPendingDevices) },
                            ) {
                                Text("Accept All", style = MaterialTheme.typography.labelSmall)
                            }
                            Button(
                                onClick = { manager.dispatch(AppAction.RejectAllPendingDevices) },
                                colors = ButtonDefaults.buttonColors(
                                    containerColor = MaterialTheme.colorScheme.error,
                                ),
                            ) {
                                Text("Reject All", style = MaterialTheme.typography.labelSmall)
                            }
                        }
                    }
                }

                items(pendingDevices, key = { it.fingerprint }) { device ->
                    PendingDeviceRow(
                        device = device,
                        onAccept = { manager.dispatch(AppAction.AcceptPendingDevice(device.fingerprint)) },
                        onReject = { manager.dispatch(AppAction.RejectPendingDevice(device.fingerprint)) },
                    )
                }
            }

            item { HorizontalDivider() }

            // Devices header
            item {
                Text(
                    "My Devices (${devices.size})",
                    style = MaterialTheme.typography.titleSmall,
                )
            }

            if (devices.isEmpty()) {
                item {
                    Text(
                        "No devices found",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                items(devices, key = { it.fingerprint }) { device ->
                    DeviceRow(device)
                }
            }
        }
    }
}

@Composable
private fun PendingDeviceRow(device: DeviceInfo, onAccept: () -> Unit, onReject: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                deviceDisplayName(device),
                style = MaterialTheme.typography.bodyLarge,
            )
            Spacer(Modifier.height(2.dp))
            Text(
                "Published: ${formatTimestamp(device.publishedAt)}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        IconButton(onClick = onAccept) {
            Icon(
                Icons.Default.Check,
                contentDescription = "Accept",
                tint = MaterialTheme.colorScheme.primary,
            )
        }
        IconButton(onClick = onReject) {
            Icon(
                Icons.Default.Close,
                contentDescription = "Reject",
                tint = MaterialTheme.colorScheme.error,
            )
        }
    }
}

@Composable
private fun DeviceRow(device: DeviceInfo) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    deviceDisplayName(device),
                    style = MaterialTheme.typography.bodyLarge,
                )
                if (device.isCurrentDevice) {
                    Text(
                        "This device",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            }
            Spacer(Modifier.height(2.dp))
            Text(
                "Published: ${formatTimestamp(device.publishedAt)}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

private fun deviceDisplayName(device: DeviceInfo): String {
    return "Device ${device.fingerprint}"
}

private fun formatTimestamp(epochSecs: Long): String {
    val sdf = SimpleDateFormat("MMM d, yyyy h:mm a", Locale.getDefault())
    return sdf.format(Date(epochSecs * 1000))
}

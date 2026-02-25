package com.pika.app.ui.screens

import android.widget.Toast
import androidx.compose.animation.core.Animatable
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Badge
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SmallFloatingActionButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.pika.app.AppManager
import com.pika.app.rust.AppAction
import com.pika.app.rust.ChatMessage
import com.pika.app.rust.MessageDeliveryState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ArrowBack
import androidx.compose.material.icons.filled.ArrowDownward
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Reply
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.unit.IntOffset
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.roundToInt
import com.pika.app.rust.Screen
import com.pika.app.ui.theme.PikaBlue
import com.pika.app.ui.TestTags
import dev.jeziellago.compose.markdowntext.MarkdownText
import kotlinx.coroutines.launch
import org.json.JSONObject

// Represents an item in the chat timeline: either a message or the unread divider.
private sealed class ChatListItem {
    data class Message(val message: ChatMessage) : ChatListItem()
    object NewMessagesDivider : ChatListItem()
}

@Composable
@OptIn(ExperimentalMaterial3Api::class)
fun ChatScreen(
    manager: AppManager,
    chatId: String,
    padding: PaddingValues,
    onOpenCallSurface: (String) -> Unit,
) {
    val chat = manager.state.currentChat
    if (chat == null || chat.chatId != chatId) {
        Box(modifier = Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
            Text("Loading chat…")
        }
        return
    }

    var draft by remember { mutableStateOf("") }
    var replyDraft by remember(chat.chatId) { mutableStateOf<ChatMessage?>(null) }
    val listState = rememberLazyListState()
    val coroutineScope = rememberCoroutineScope()
    val newestMessageId = chat.messages.lastOrNull()?.id
    var shouldStickToBottom by remember(chat.chatId) { mutableStateOf(true) }
    var programmaticScrollInFlight by remember { mutableStateOf(false) }
    val isAtBottom by remember(listState) {
        derivedStateOf { listState.isNearBottomForReverseLayout() }
    }

    // Capture unread count once when this chat is first opened, so we know where to draw
    // the "NEW MESSAGES" divider and can scroll to it on entry.
    val capturedUnreadCount = remember(chat.chatId) {
        manager.state.chatList.find { it.chatId == chatId }?.unreadCount?.toInt() ?: 0
    }

    // Track new messages arriving while the user is scrolled up.
    var newMessageCount by remember(chat.chatId) { mutableIntStateOf(0) }
    var prevMessageCount by remember(chat.chatId) { mutableIntStateOf(chat.messages.size) }

    val myPubkey =
        when (val a = manager.state.auth) {
            is com.pika.app.rust.AuthState.LoggedIn -> a.pubkey
            else -> null
        }
    val title = chatTitle(chat, myPubkey)
    val activeCall = manager.state.activeCall
    val callForChat = activeCall?.takeIf { it.chatId == chat.chatId }
    val hasLiveCallElsewhere = activeCall?.let { it.chatId != chat.chatId && it.isLive } ?: false
    val isCallActionDisabled = callForChat == null && hasLiveCallElsewhere
    val messagesById = remember(chat.messages) { chat.messages.associateBy { it.id } }
    val reversed = remember(chat.messages) { chat.messages.asReversed() }
    val reversedIndexById =
        remember(reversed) { reversed.mapIndexed { index, message -> message.id to index }.toMap() }

    // Build the display list, inserting the "NEW MESSAGES" divider between read and unread.
    val listItems: List<ChatListItem> = remember(reversed, capturedUnreadCount) {
        buildList {
            for ((i, msg) in reversed.withIndex()) {
                // Insert divider before the first read message (i.e. after all unread messages).
                if (i == capturedUnreadCount && capturedUnreadCount > 0 && capturedUnreadCount < reversed.size) {
                    add(ChatListItem.NewMessagesDivider)
                }
                add(ChatListItem.Message(msg))
            }
        }
    }

    // On chat open: if there are unreads, scroll to the divider; otherwise scroll to newest.
    LaunchedEffect(chat.chatId) {
        if (capturedUnreadCount > 0 && reversed.isNotEmpty()) {
            shouldStickToBottom = false
            // Scroll so the divider is visible (it sits at index capturedUnreadCount in listItems).
            val dividerIndex = minOf(capturedUnreadCount, listItems.size - 1)
            listState.scrollToItem(dividerIndex)
        } else if (chat.messages.isNotEmpty()) {
            listState.scrollToItem(0)
        }
        replyDraft = null
    }

    LaunchedEffect(isAtBottom, listState.isScrollInProgress, programmaticScrollInFlight) {
        if (isAtBottom) {
            shouldStickToBottom = true
            newMessageCount = 0
        } else if (listState.isScrollInProgress && !programmaticScrollInFlight) {
            shouldStickToBottom = false
        }
    }

    // Scroll to newest when a new message arrives and we're stuck to the bottom.
    LaunchedEffect(newestMessageId) {
        if (newestMessageId == null || !shouldStickToBottom) return@LaunchedEffect
        programmaticScrollInFlight = true
        try {
            listState.animateScrollToItem(0)
        } finally {
            programmaticScrollInFlight = false
        }
    }

    // Track new messages arriving while scrolled up, for the badge.
    LaunchedEffect(reversed.size) {
        val current = reversed.size
        if (current > prevMessageCount && !shouldStickToBottom) {
            newMessageCount += current - prevMessageCount
        }
        prevMessageCount = current
    }

    // When the keyboard appears, scroll to the newest message so it stays visible above the input.
    val imeBottom = WindowInsets.ime.getBottom(LocalDensity.current)
    LaunchedEffect(imeBottom) {
        if (imeBottom > 0 && shouldStickToBottom) {
            coroutineScope.launch {
                listState.animateScrollToItem(0)
            }
        }
    }

    Scaffold(
        modifier = Modifier.padding(padding),
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = title,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
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
                actions = {
                    IconButton(
                        onClick = { onOpenCallSurface(chat.chatId) },
                        enabled = !isCallActionDisabled,
                        modifier =
                            Modifier.testTag(
                                if (callForChat?.isLive == true) {
                                    TestTags.CHAT_CALL_OPEN
                                } else {
                                    TestTags.CHAT_CALL_START
                                },
                            ),
                    ) {
                        Icon(Icons.Default.Call, contentDescription = "Call")
                    }

                    if (chat.isGroup) {
                        IconButton(
                            onClick = {
                                manager.dispatch(AppAction.PushScreen(Screen.GroupInfo(chat.chatId)))
                            },
                        ) {
                            Icon(Icons.Default.Info, contentDescription = "Group info")
                        }
                    } else {
                        // 1:1 chat — show info button to open the contact's profile (and copy npub).
                        val peer = chat.members.firstOrNull { it.pubkey != myPubkey }
                            ?: chat.members.firstOrNull()
                        if (peer != null) {
                            IconButton(
                                onClick = {
                                    manager.dispatch(AppAction.OpenPeerProfile(peer.pubkey))
                                },
                            ) {
                                Icon(Icons.Default.Info, contentDescription = "Contact info")
                            }
                        }
                    }
                },
            )
        },
    ) { inner ->
        Column(
            modifier =
                Modifier
                    .fillMaxSize()
                    .padding(inner)
                    .padding(top = 8.dp),
        ) {
            // Wrap the message list in a Box so we can overlay the scroll-to-bottom button.
            Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize().testTag(TestTags.CHAT_MESSAGE_LIST),
                    reverseLayout = true,
                    contentPadding = PaddingValues(horizontal = 12.dp, vertical = 10.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    items(
                        items = listItems,
                        key = { item ->
                            when (item) {
                                is ChatListItem.Message -> item.message.id
                                is ChatListItem.NewMessagesDivider -> "new-messages-divider"
                            }
                        },
                    ) { item ->
                        when (item) {
                            is ChatListItem.Message -> {
                                val msg = item.message
                                MessageBubble(
                                    message = msg,
                                    messagesById = messagesById,
                                    onSendMessage = { text ->
                                        manager.dispatch(AppAction.SendMessage(chat.chatId, text, null, null))
                                    },
                                    onReplyTo = { replyMessage ->
                                        replyDraft = replyMessage
                                    },
                                    onJumpToMessage = { targetId ->
                                        val index = reversedIndexById[targetId] ?: return@MessageBubble
                                        coroutineScope.launch {
                                            listState.animateScrollToItem(index)
                                        }
                                    },
                                )
                            }
                            is ChatListItem.NewMessagesDivider -> {
                                NewMessagesDividerRow()
                            }
                        }
                    }
                }

                // Scroll-to-bottom button with new message count badge.
                if (!isAtBottom) {
                    Column(
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .padding(end = 16.dp, bottom = 12.dp),
                        horizontalAlignment = Alignment.End,
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        if (newMessageCount > 0) {
                            Badge(
                                containerColor = PikaBlue,
                            ) {
                                Text(
                                    text = "$newMessageCount new",
                                    style = MaterialTheme.typography.labelSmall,
                                )
                            }
                        }
                        SmallFloatingActionButton(
                            onClick = {
                                shouldStickToBottom = true
                                newMessageCount = 0
                                coroutineScope.launch { listState.animateScrollToItem(0) }
                            },
                            containerColor = MaterialTheme.colorScheme.surfaceVariant,
                            contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        ) {
                            Icon(Icons.Default.ArrowDownward, contentDescription = "Scroll to bottom")
                        }
                    }
                }
            }

            Column(
                modifier =
                    Modifier
                        .fillMaxWidth()
                        .navigationBarsPadding()
                        .imePadding()
                        .padding(start = 12.dp, top = 8.dp, end = 12.dp, bottom = 16.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                replyDraft?.let { replying ->
                    ReplyComposerPreview(
                        message = replying,
                        onClear = { replyDraft = null },
                    )
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    OutlinedTextField(
                        value = draft,
                        onValueChange = { draft = it },
                        modifier = Modifier.weight(1f).testTag(TestTags.CHAT_MESSAGE_INPUT),
                        placeholder = { Text("Message") },
                        singleLine = false,
                        maxLines = 4,
                    )
                    Spacer(Modifier.width(10.dp))
                    Button(
                        onClick = {
                            val text = draft
                            draft = ""
                            manager.dispatch(
                                AppAction.SendMessage(chat.chatId, text, null, replyDraft?.id),
                            )
                            replyDraft = null
                        },
                        enabled = draft.isNotBlank(),
                        modifier = Modifier.testTag(TestTags.CHAT_SEND),
                    ) {
                        Text("Send")
                    }
                }
            }
        }
    }
}

private fun LazyListState.isNearBottomForReverseLayout(tolerancePx: Int = 100): Boolean {
    if (firstVisibleItemIndex != 0) return false
    return firstVisibleItemScrollOffset <= tolerancePx
}

private fun chatTitle(chat: com.pika.app.rust.ChatViewState, selfPubkey: String?): String {
    if (chat.isGroup) {
        return chat.groupName?.trim().takeIf { !it.isNullOrBlank() } ?: "Group chat"
    }
    val peer =
        chat.members.firstOrNull { selfPubkey == null || it.pubkey != selfPubkey }
            ?: chat.members.firstOrNull()
    return peer?.name?.trim().takeIf { !it.isNullOrBlank() } ?: peer?.npub ?: "Chat"
}

// Parsed segment of a message: either markdown text or a pika-* custom block.
private sealed class MessageSegment {
    data class Markdown(val text: String) : MessageSegment()
    data class PikaPrompt(val title: String, val options: List<String>) : MessageSegment()
    data class PikaHtml(val html: String) : MessageSegment()
}

private fun parseMessageSegments(content: String): List<MessageSegment> {
    val segments = mutableListOf<MessageSegment>()
    val pattern = Regex("```pika-([\\w-]+)(?:[ \\t]+(\\S+))?\\n([\\s\\S]*?)```")
    var lastEnd = 0

    for (match in pattern.findAll(content)) {
        val before = content.substring(lastEnd, match.range.first)
        if (before.isNotBlank()) segments.add(MessageSegment.Markdown(before))

        val blockType = match.groupValues[1]
        val blockBody = match.groupValues[3].trim()

        when (blockType) {
            "prompt" -> {
                try {
                    val json = JSONObject(blockBody)
                    val title = json.getString("title")
                    val optionsArray = json.getJSONArray("options")
                    val options = (0 until optionsArray.length()).map { optionsArray.getString(it) }
                    segments.add(MessageSegment.PikaPrompt(title, options))
                } catch (_: Exception) {
                    segments.add(MessageSegment.Markdown("```$blockType\n$blockBody\n```"))
                }
            }
            "html" -> {
                segments.add(MessageSegment.PikaHtml(blockBody))
            }
            "html-update", "prompt-response" -> {
                // Consumed by Rust core; silently drop if one slips through.
            }
            else -> {
                segments.add(MessageSegment.Markdown("```$blockType\n$blockBody\n```"))
            }
        }

        lastEnd = match.range.last + 1
    }

    val tail = content.substring(lastEnd)
    if (tail.isNotBlank()) segments.add(MessageSegment.Markdown(tail))

    return segments
}

@Composable
private fun NewMessagesDividerRow() {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 10.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        HorizontalDivider(
            modifier = Modifier.weight(1f),
            color = PikaBlue.copy(alpha = 0.35f),
        )
        Text(
            text = "NEW MESSAGES",
            style = MaterialTheme.typography.labelSmall,
            color = PikaBlue.copy(alpha = 0.8f),
        )
        HorizontalDivider(
            modifier = Modifier.weight(1f),
            color = PikaBlue.copy(alpha = 0.35f),
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MessageBubble(
    message: ChatMessage,
    messagesById: Map<String, ChatMessage>,
    onSendMessage: (String) -> Unit,
    onReplyTo: (ChatMessage) -> Unit,
    onJumpToMessage: (String) -> Unit,
) {
    val isMine = message.isMine
    val bubbleColor = if (isMine) PikaBlue else MaterialTheme.colorScheme.surfaceVariant
    val textColor = if (isMine) Color.White else MaterialTheme.colorScheme.onSurfaceVariant
    val align = if (isMine) Alignment.End else Alignment.Start
    val segments = remember(message.displayContent) { parseMessageSegments(message.displayContent) }
    val ctx = LocalContext.current
    val clipboardManager = LocalClipboardManager.current
    val haptic = LocalHapticFeedback.current
    val coroutineScope = rememberCoroutineScope()
    val replyTarget = remember(message.replyToMessageId, messagesById) {
        message.replyToMessageId?.let { messagesById[it] }
    }
    val formattedTime = remember(message.timestamp) {
        SimpleDateFormat("MMM d, h:mm a", Locale.getDefault()).format(Date(message.timestamp * 1000L))
    }
    // Swipe-to-reply state
    val swipeOffset = remember { Animatable(0f) }
    val swipeThreshold = 80f
    var replyTriggered by remember { mutableStateOf(false) }

    Column(modifier = Modifier.fillMaxWidth(), horizontalAlignment = align) {
        message.replyToMessageId?.let { replyToMessageId ->
            ReplyReferencePreview(
                replyToMessageId = replyToMessageId,
                target = replyTarget,
                isMine = isMine,
                onJumpToMessage = onJumpToMessage,
            )
            Spacer(Modifier.height(4.dp))
        }

        for (segment in segments) {
            when (segment) {
                is MessageSegment.Markdown -> {
                    var showTimestamp by remember { mutableStateOf(false) }
                    Box(
                        modifier = Modifier
                            .fillMaxWidth()
                            .draggable(
                                orientation = Orientation.Horizontal,
                                state = rememberDraggableState { delta ->
                                    // Only allow rightward swipe up to threshold + a bit of resistance
                                    val newOffset = (swipeOffset.value + delta).coerceIn(0f, swipeThreshold * 1.2f)
                                    coroutineScope.launch { swipeOffset.snapTo(newOffset) }
                                    if (swipeOffset.value >= swipeThreshold && !replyTriggered) {
                                        replyTriggered = true
                                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                                    }
                                },
                                onDragStopped = {
                                    if (replyTriggered) {
                                        onReplyTo(message)
                                        replyTriggered = false
                                    }
                                    coroutineScope.launch { swipeOffset.animateTo(0f) }
                                },
                            ),
                    ) {
                        // Reply icon revealed behind the bubble as it swipes
                        if (swipeOffset.value > 8f) {
                            Icon(
                                Icons.Default.Reply,
                                contentDescription = "Reply",
                                tint = PikaBlue.copy(alpha = (swipeOffset.value / swipeThreshold).coerceIn(0f, 1f)),
                                modifier = Modifier
                                    .align(Alignment.CenterStart)
                                    .padding(start = 8.dp)
                                    .size(20.dp),
                            )
                        }
                        Row(
                            verticalAlignment = Alignment.Bottom,
                            modifier = Modifier
                                .fillMaxWidth()
                                .offset { IntOffset(swipeOffset.value.roundToInt(), 0) },
                            horizontalArrangement = if (isMine) Arrangement.End else Arrangement.Start,
                        ) {
                            // Use a Box to anchor the DropdownMenu to the bubble.
                            var showMenu by remember { mutableStateOf(false) }
                            Box {
                                Box(
                                    modifier =
                                        Modifier
                                            .clip(RoundedCornerShape(18.dp))
                                            .background(bubbleColor)
                                            .combinedClickable(
                                                onClick = { showTimestamp = !showTimestamp },
                                                onLongClick = {
                                                    showMenu = true
                                                },
                                            )
                                            .padding(horizontal = 12.dp, vertical = 9.dp)
                                            .widthIn(max = 280.dp),
                                ) {
                                    MarkdownText(
                                        markdown = segment.text.trim(),
                                        style = MaterialTheme.typography.bodyLarge.copy(color = textColor),
                                        enableSoftBreakAddsNewLine = true,
                                        afterSetMarkdown = { textView ->
                                            textView.includeFontPadding = false
                                        },
                                    )
                                }
                                DropdownMenu(
                                    expanded = showMenu,
                                    onDismissRequest = { showMenu = false },
                                ) {
                                    DropdownMenuItem(
                                        text = { Text("Reply") },
                                        onClick = {
                                            onReplyTo(message)
                                            showMenu = false
                                        },
                                    )
                                    DropdownMenuItem(
                                        text = { Text("Copy text") },
                                        onClick = {
                                            clipboardManager.setText(AnnotatedString(message.displayContent))
                                            Toast.makeText(ctx, "Copied", Toast.LENGTH_SHORT).show()
                                            showMenu = false
                                        },
                                    )
                                }
                            }
                            if (isMine) {
                                Spacer(Modifier.width(6.dp))
                                Text(
                                    text =
                                        when (message.delivery) {
                                            is MessageDeliveryState.Pending -> "…"
                                            is MessageDeliveryState.Sent -> "✓"
                                            is MessageDeliveryState.Failed -> "!"
                                        },
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    style = MaterialTheme.typography.labelSmall,
                                )
                            }
                        }
                    }
                    if (showTimestamp) {
                        Text(
                            text = formattedTime,
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            textAlign = if (isMine) TextAlign.End else TextAlign.Start,
                            modifier = Modifier
                                .fillMaxWidth()
                                .padding(horizontal = 14.dp, vertical = 2.dp),
                        )
                    }
                }
                is MessageSegment.PikaPrompt -> {
                    PikaPromptCard(
                        title = segment.title,
                        options = segment.options,
                        message = message,
                        onSelect = onSendMessage,
                    )
                }
                is MessageSegment.PikaHtml -> {
                    Box(
                        modifier =
                            Modifier
                                .clip(RoundedCornerShape(16.dp))
                                .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                                .padding(12.dp)
                                .widthIn(max = 280.dp),
                    ) {
                        MarkdownText(
                            markdown = segment.html,
                            style = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurfaceVariant),
                            enableSoftBreakAddsNewLine = true,
                            afterSetMarkdown = { textView ->
                                textView.includeFontPadding = false
                            },
                        )
                    }
                }
            }
        }
        Spacer(Modifier.height(2.dp))
    }
}

@Composable
private fun ReplyReferencePreview(
    replyToMessageId: String,
    target: ChatMessage?,
    isMine: Boolean,
    onJumpToMessage: (String) -> Unit,
) {
    val sender = remember(target) {
        when {
            target == null -> "Original message"
            target.isMine -> "You"
            !target.senderName.isNullOrBlank() -> target.senderName!!
            else -> target.senderPubkey.take(8)
        }
    }
    val snippet = remember(target) {
        val text = target?.displayContent?.trim().orEmpty()
        when {
            target == null -> "Original message not loaded"
            text.isEmpty() -> "(empty message)"
            else -> text.lineSequence().first().let { first ->
                if (first.length > 80) first.take(80) + "…" else first
            }
        }
    }

    val modifier =
        Modifier
            .clip(RoundedCornerShape(10.dp))
            .background(if (isMine) Color.White.copy(alpha = 0.14f) else Color.Black.copy(alpha = 0.08f))
            .padding(horizontal = 10.dp, vertical = 6.dp)
            .widthIn(max = 280.dp)

    Row(
        modifier =
            if (target != null) {
                modifier.clickable { onJumpToMessage(replyToMessageId) }
            } else {
                modifier
            },
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier.width(2.dp).height(28.dp).background(if (isMine) Color.White.copy(alpha = 0.8f) else PikaBlue),
        )
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = sender,
                style = MaterialTheme.typography.labelSmall,
                color = if (isMine) Color.White.copy(alpha = 0.86f) else MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
            )
            Text(
                text = snippet,
                style = MaterialTheme.typography.bodySmall,
                color = if (isMine) Color.White.copy(alpha = 0.8f) else MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun ReplyComposerPreview(
    message: ChatMessage,
    onClear: () -> Unit,
) {
    val sender =
        when {
            message.isMine -> "You"
            !message.senderName.isNullOrBlank() -> message.senderName!!
            else -> message.senderPubkey.take(8)
        }
    val snippet =
        message.displayContent.trim().lineSequence().firstOrNull()?.let {
            if (it.length > 80) it.take(80) + "…" else it
        } ?: "(empty message)"

    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(10.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.35f))
                .padding(horizontal = 10.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(
            modifier = Modifier.width(2.dp).height(28.dp).background(PikaBlue),
        )
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = "Replying to $sender",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
            )
            Text(
                text = snippet,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        TextButton(onClick = onClear) {
            Text("Cancel")
        }
    }
}

@Composable
private fun PikaPromptCard(title: String, options: List<String>, message: ChatMessage, onSelect: (String) -> Unit) {
    val hasVoted = message.myPollVote != null
    Column(
        modifier =
            Modifier
                .clip(RoundedCornerShape(16.dp))
                .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                .padding(12.dp)
                .widthIn(max = 280.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurface,
        )
        for (option in options) {
            val tally = message.pollTally.firstOrNull { it.option == option }
            val isMyVote = message.myPollVote == option
            TextButton(
                onClick = {
                    val response = "```pika-prompt-response\n{\"prompt_id\":\"${message.id}\",\"selected\":\"$option\"}\n```"
                    onSelect(response)
                },
                enabled = !hasVoted,
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(10.dp),
                colors =
                    ButtonDefaults.textButtonColors(
                        containerColor = if (isMyVote) PikaBlue.copy(alpha = 0.25f) else PikaBlue.copy(alpha = 0.1f),
                        contentColor = PikaBlue,
                        disabledContainerColor = if (isMyVote) PikaBlue.copy(alpha = 0.25f) else PikaBlue.copy(alpha = 0.1f),
                        disabledContentColor = PikaBlue.copy(alpha = 0.7f),
                    ),
            ) {
                Row(modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                    Text(option)
                    if (tally != null) {
                        Text("${tally.count}", style = MaterialTheme.typography.titleSmall)
                    }
                }
            }
            if (tally != null && tally.voterNames.isNotEmpty()) {
                Text(
                    text = tally.voterNames.joinToString(", "),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 12.dp),
                )
            }
        }
    }
}

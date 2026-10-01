package websocket

import (
	"encoding/json"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/gorilla/websocket"
)

// Event types for WebSocket messages
const (
	// Content updates
	EventContentUpdate   = "CONTENT_UPDATE"   // When new content is created
	EventContentVote     = "CONTENT_VOTE"     // When content receives a vote
	EventContentComment  = "CONTENT_COMMENT"  // When content receives a comment
	EventContentEvidence = "CONTENT_EVIDENCE" // When evidence is added
	EventConsensusUpdate = "CONSENSUS_UPDATE" // When consensus state changes

	// Direct messages
	EventDirectMessage = "DIRECT_MESSAGE" // Private message between users
	EventMessageRead   = "MESSAGE_READ"   // Message read receipt
	EventMessageTyping = "MESSAGE_TYPING" // Typing indicator

	// Notifications
	EventNotifyMention    = "NOTIFY_MENTION"    // User mention notification
	EventNotifyReply      = "NOTIFY_REPLY"      // Reply to content/comment
	EventNotifyConsensus  = "NOTIFY_CONSENSUS"  // Consensus reached on user's content
	EventNotifyEvidence   = "NOTIFY_EVIDENCE"   // New evidence on user's content
	EventNotifyReputation = "NOTIFY_REPUTATION" // Reputation change notification
)

// WebSocketMessage represents a structured message
type WebSocketMessage struct {
	Type      string      `json:"type"`
	Data      interface{} `json:"data"`
	Timestamp time.Time   `json:"timestamp"`
	UserID    string      `json:"user_id,omitempty"`   // Sender's user ID
	TargetID  string      `json:"target_id,omitempty"` // Target user ID for DMs
}

// WebSocketHub maintains the set of active clients
type WebSocketHub struct {
	// Registered clients mapped by user ID
	clients    map[string]*WebSocketClient
	broadcast  chan *WebSocketMessage
	register   chan *WebSocketClient
	unregister chan *WebSocketClient
	mu         sync.RWMutex
}

// WebSocketClient represents a connected client
type WebSocketClient struct {
	hub     *WebSocketHub
	conn    *websocket.Conn
	send    chan *WebSocketMessage
	userID  string
	isAlive bool
	mu      sync.RWMutex
}

var upgrader = websocket.Upgrader{
	ReadBufferSize:  1024,
	WriteBufferSize: 1024,
	CheckOrigin: func(r *http.Request) bool {
		return true // Allow all origins for now
	},
}

// NewWebSocketHub creates a new WebSocketHub
func NewWebSocketHub() *WebSocketHub {
	return &WebSocketHub{
		broadcast:  make(chan *WebSocketMessage),
		register:   make(chan *WebSocketClient),
		unregister: make(chan *WebSocketClient),
		clients:    make(map[string]*WebSocketClient),
	}
}

// Run starts the WebSocketHub
func (h *WebSocketHub) Run() {
	for {
		select {
		case client := <-h.register:
			h.mu.Lock()
			h.clients[client.userID] = client
			h.mu.Unlock()

		case client := <-h.unregister:
			h.mu.Lock()
			if _, ok := h.clients[client.userID]; ok {
				delete(h.clients, client.userID)
				close(client.send)
			}
			h.mu.Unlock()

		case message := <-h.broadcast:
			h.mu.RLock()
			switch message.Type {
			case EventDirectMessage:
				// Send to specific user
				if client, ok := h.clients[message.TargetID]; ok {
					select {
					case client.send <- message:
					default:
						close(client.send)
						delete(h.clients, client.userID)
					}
				}
			default:
				// Broadcast to all clients
				for _, client := range h.clients {
					select {
					case client.send <- message:
					default:
						close(client.send)
						delete(h.clients, client.userID)
					}
				}
			}
			h.mu.RUnlock()
		}
	}
}

// HandleWebSocket handles websocket requests from clients
func (h *WebSocketHub) HandleWebSocket(c *gin.Context) {
	userID := c.GetString("user_id") // Get from auth middleware
	if userID == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "Unauthorized"})
		return
	}

	conn, err := upgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		log.Printf("Error upgrading connection: %v", err)
		return
	}

	client := &WebSocketClient{
		hub:     h,
		conn:    conn,
		send:    make(chan *WebSocketMessage, 256),
		userID:  userID,
		isAlive: true,
	}

	client.hub.register <- client

	// Start client goroutines
	go client.writePump()
	go client.readPump()
	go client.pingPump()
}

// writePump pumps messages from the hub to the websocket connection
func (c *WebSocketClient) writePump() {
	ticker := time.NewTicker(pingPeriod)
	defer func() {
		ticker.Stop()
		c.conn.Close()
	}()

	for {
		select {
		case message, ok := <-c.send:
			if !ok {
				c.conn.WriteMessage(websocket.CloseMessage, []byte{})
				return
			}

			w, err := c.conn.NextWriter(websocket.TextMessage)
			if err != nil {
				return
			}

			jsonData, err := json.Marshal(message)
			if err != nil {
				return
			}

			w.Write(jsonData)
			w.Close()

		case <-ticker.C:
			if err := c.conn.WriteMessage(websocket.PingMessage, nil); err != nil {
				return
			}
		}
	}
}

// readPump pumps messages from the websocket connection to the hub
func (c *WebSocketClient) readPump() {
	defer func() {
		c.hub.unregister <- c
		c.conn.Close()
	}()

	c.conn.SetReadLimit(maxMessageSize)
	c.conn.SetReadDeadline(time.Now().Add(pongWait))
	c.conn.SetPongHandler(func(string) error {
		c.mu.Lock()
		c.isAlive = true
		c.mu.Unlock()
		c.conn.SetReadDeadline(time.Now().Add(pongWait))
		return nil
	})

	for {
		_, data, err := c.conn.ReadMessage()
		if err != nil {
			if websocket.IsUnexpectedCloseError(err, websocket.CloseGoingAway, websocket.CloseAbnormalClosure) {
				log.Printf("error: %v", err)
			}
			break
		}

		var message WebSocketMessage
		if err := json.Unmarshal(data, &message); err != nil {
			continue
		}

		message.Timestamp = time.Now()
		message.UserID = c.userID // Set sender's user ID
		c.hub.broadcast <- &message
	}
}

// pingPump sends periodic pings to client
func (c *WebSocketClient) pingPump() {
	ticker := time.NewTicker(pingPeriod)
	defer ticker.Stop()

	for {
		<-ticker.C
		c.mu.RLock()
		isAlive := c.isAlive
		c.mu.RUnlock()

		if !isAlive {
			c.hub.unregister <- c
			return
		}

		c.mu.Lock()
		c.isAlive = false
		c.mu.Unlock()
	}
}

// SendToUser sends a message to a specific user
func (h *WebSocketHub) SendToUser(targetUserID string, messageType string, data interface{}) {
	message := &WebSocketMessage{
		Type:      messageType,
		Data:      data,
		Timestamp: time.Now(),
		TargetID:  targetUserID,
	}
	h.broadcast <- message
}

// BroadcastUpdate sends an update to all connected clients
func (h *WebSocketHub) BroadcastUpdate(messageType string, data interface{}) {
	message := &WebSocketMessage{
		Type:      messageType,
		Data:      data,
		Timestamp: time.Now(),
	}
	h.broadcast <- message
}

const (
	writeWait      = 10 * time.Second
	pongWait       = 60 * time.Second
	pingPeriod     = (pongWait * 9) / 10
	maxMessageSize = 512 * 1024
)

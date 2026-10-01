package handlers

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"

	"github.com/saint/babel-protocol/backend/api/models"
	"github.com/saint/babel-protocol/backend/internal/db/sqlite"
	"github.com/saint/babel-protocol/backend/internal/websocket"
)

// CreateContentHandler handles the creation of new content
func CreateContentHandler(hub *websocket.WebSocketHub, db *sqlite.DBManager) gin.HandlerFunc {
	return func(c *gin.Context) {
		var content models.Content
		if err := c.ShouldBindJSON(&content); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Set content metadata
		content.AuthorID = c.GetString("user_id")
		content.Timestamp = time.Now()
		content.LastUpdated = time.Now()
		content.ProcessingStatus = "pending"

		// Generate unique ID for content
		content.ID = uuid.New().String()

		// Save content to database
		if err := db.CreateContent(&content); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Broadcast new content to all users
		hub.BroadcastUpdate(websocket.EventContentUpdate, content)

		c.JSON(http.StatusCreated, content)
	}
}

// GetContentHandler handles retrieving content by ID
func GetContentHandler() gin.HandlerFunc {
	return func(c *gin.Context) {
		id := c.Param("id")
		// TODO: Implement content retrieval logic
		c.JSON(http.StatusOK, gin.H{"id": id})
	}
}

// VoteContentHandler handles voting on content
func VoteContentHandler(hub *websocket.WebSocketHub, db *sqlite.DBManager) gin.HandlerFunc {
	return func(c *gin.Context) {
		contentID := c.Param("id")
		var vote models.Vote
		if err := c.ShouldBindJSON(&vote); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Set vote metadata
		vote.ID = uuid.New().String()
		vote.ContentID = contentID
		vote.UserID = c.GetString("user_id")
		vote.Timestamp = time.Now()
		vote.LastUpdated = time.Now()

		// Validate vote
		if err := vote.Validate(); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Record vote in consensus system
		if err := db.RecordVote(
			vote.ContentID,
			vote.UserID,
			vote.Type,
			vote.Weight,
			vote.CertaintyLevel,
			vote.EvidenceIDs,
		); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Get content author from database
		content, err := db.GetContent(contentID)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Notify content author
		hub.SendToUser(content.AuthorID, websocket.EventContentVote, vote)

		// Get updated vote count
		votes, err := db.GetContentVotes(contentID)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Broadcast vote update to all users
		hub.BroadcastUpdate(websocket.EventContentVote, map[string]interface{}{
			"content_id": contentID,
			"vote_type":  vote.Type,
			"count":      len(votes),
		})

		c.JSON(http.StatusOK, vote)
	}
}

// CommentContentHandler handles adding comments to content
func CommentContentHandler(hub *websocket.WebSocketHub, db *sqlite.DBManager) gin.HandlerFunc {
	return func(c *gin.Context) {
		contentID := c.Param("id")
		var comment models.Comment
		if err := c.ShouldBindJSON(&comment); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Set comment metadata
		comment.ID = uuid.New().String()
		comment.ContentID = contentID
		comment.AuthorID = c.GetString("user_id")
		comment.Timestamp = time.Now()

		// Create comment as content
		content := &models.Content{
			ID:          comment.ID,
			AuthorID:    comment.AuthorID,
			ContentType: "comment",
			ContentText: comment.Text,
			ParentID:    &contentID,
			Timestamp:   comment.Timestamp,
			LastUpdated: comment.Timestamp,
		}

		// Save comment to database
		if err := db.CreateContent(content); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Get parent content author
		parentContent, err := db.GetContent(contentID)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Notify content author
		hub.SendToUser(parentContent.AuthorID, websocket.EventNotifyReply, comment)

		// Broadcast comment to all users viewing the content
		hub.BroadcastUpdate(websocket.EventContentComment, comment)

		c.JSON(http.StatusOK, comment)
	}
}

// AddContextHandler handles adding context posts to content
func AddContextHandler(hub *websocket.WebSocketHub, db *sqlite.DBManager) gin.HandlerFunc {
	return func(c *gin.Context) {
		parentID := c.Param("id")
		var contextPost models.Content
		if err := c.ShouldBindJSON(&contextPost); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Get parent content
		parentContent, err := db.GetContent(parentID)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Set context post metadata
		contextPost.ID = uuid.New().String()
		contextPost.AuthorID = c.GetString("user_id")
		contextPost.ContentType = "post"
		contextPost.ParentID = &parentID
		contextPost.IsContext = true // Mark as context post
		contextPost.Timestamp = time.Now()
		contextPost.LastUpdated = time.Now()
		contextPost.ProcessingStatus = "pending"

		// Save context post to database
		if err := db.CreateContent(&contextPost); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Notify parent content author
		hub.SendToUser(parentContent.AuthorID, websocket.EventContentEvidence, contextPost)

		// Broadcast context addition to all users
		hub.BroadcastUpdate(websocket.EventContentEvidence, map[string]interface{}{
			"parent_id":  parentID,
			"context_id": contextPost.ID,
			"author_id":  contextPost.AuthorID,
			"timestamp":  contextPost.Timestamp,
		})

		c.JSON(http.StatusOK, contextPost)
	}
}

// SendDirectMessageHandler handles sending direct messages between users
func SendDirectMessageHandler(hub *websocket.WebSocketHub, db *sqlite.DBManager) gin.HandlerFunc {
	return func(c *gin.Context) {
		targetUserID := c.Param("user_id")
		var message models.DirectMessage
		if err := c.ShouldBindJSON(&message); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Set message metadata
		message.ID = uuid.New().String()
		message.SenderID = c.GetString("user_id")
		message.ReceiverID = targetUserID
		message.Timestamp = time.Now()

		// Save message to database
		if err := db.CreateDirectMessage(&message); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Send message to target user
		hub.SendToUser(targetUserID, websocket.EventDirectMessage, message)

		c.JSON(http.StatusOK, message)
	}
}

// UpdateConsensusHandler handles consensus updates
func UpdateConsensusHandler(hub *websocket.WebSocketHub, db *sqlite.DBManager) gin.HandlerFunc {
	return func(c *gin.Context) {
		contentID := c.Param("id")
		var consensus models.ConsensusUpdate
		if err := c.ShouldBindJSON(&consensus); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Get content
		content, err := db.GetContent(contentID)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Set consensus metadata
		consensus.ContentID = contentID
		consensus.Timestamp = time.Now()

		// Update content consensus state
		content.Consensus.State = consensus.State
		content.Consensus.Score = consensus.Score
		content.Consensus.LastUpdated = consensus.Timestamp
		content.Consensus.ValidatorCount = consensus.Participants

		// Save updated content
		if err := db.UpdateContent(content); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Notify content author
		hub.SendToUser(content.AuthorID, websocket.EventNotifyConsensus, consensus)

		// Broadcast consensus update
		hub.BroadcastUpdate(websocket.EventConsensusUpdate, consensus)

		c.JSON(http.StatusOK, consensus)
	}
}

// UpdateReputationHandler handles reputation updates
func UpdateReputationHandler(hub *websocket.WebSocketHub, db *sqlite.DBManager) gin.HandlerFunc {
	return func(c *gin.Context) {
		userID := c.Param("id")
		var repUpdate models.ReputationUpdate
		if err := c.ShouldBindJSON(&repUpdate); err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
			return
		}

		// Get user
		user, err := db.GetUser(userID)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Set update metadata
		repUpdate.UserID = userID
		repUpdate.OldScore = user.ReputationScore
		repUpdate.Timestamp = time.Now()

		// Update user reputation
		user.ReputationScore = repUpdate.NewScore
		user.LastActive = time.Now()

		// Save updated user
		if err := db.UpdateUser(user); err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		// Notify user of reputation change
		hub.SendToUser(userID, websocket.EventNotifyReputation, repUpdate)

		c.JSON(http.StatusOK, repUpdate)
	}
}

// GetUserProfileHandler handles retrieving user profiles
func GetUserProfileHandler() gin.HandlerFunc {
	return func(c *gin.Context) {
		// TODO: Implement user profile retrieval logic
		c.JSON(http.StatusOK, gin.H{"message": "Profile retrieved"})
	}
}

// GetUserReputationHandler handles retrieving user reputation
func GetUserReputationHandler() gin.HandlerFunc {
	return func(c *gin.Context) {
		// TODO: Implement reputation retrieval logic
		c.JSON(http.StatusOK, gin.H{"message": "Reputation retrieved"})
	}
}

// GetContentAnalyticsHandler handles retrieving content analytics
func GetContentAnalyticsHandler() gin.HandlerFunc {
	return func(c *gin.Context) {
		id := c.Param("id")
		// TODO: Implement content analytics logic
		c.JSON(http.StatusOK, gin.H{"id": id})
	}
}

// GetTrendingContentHandler handles retrieving trending content
func GetTrendingContentHandler() gin.HandlerFunc {
	return func(c *gin.Context) {
		// TODO: Implement trending content logic
		c.JSON(http.StatusOK, gin.H{"message": "Trending content retrieved"})
	}
}

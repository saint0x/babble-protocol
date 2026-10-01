package main

import (
	"log"

	"github.com/gin-gonic/gin"
	"github.com/saint/babel-protocol/backend/api/handlers"
	"github.com/saint/babel-protocol/backend/api/middleware"
	"github.com/saint/babel-protocol/backend/internal/db/sqlite"
	"github.com/saint/babel-protocol/backend/internal/websocket"
)

func main() {
	// Initialize WebSocket hub
	hub := websocket.NewWebSocketHub()
	go hub.Run()

	// Initialize database manager
	dbManager, err := sqlite.NewDBManager("babel.db")
	if err != nil {
		log.Fatal("Failed to initialize database:", err)
	}

	// Set up Gin router
	router := gin.Default()

	// Add middleware
	router.Use(middleware.LoggerMiddleware())
	router.Use(middleware.CORSMiddleware())

	// WebSocket endpoint with auth
	router.GET("/ws", middleware.AuthMiddleware(), func(c *gin.Context) {
		hub.HandleWebSocket(c)
	})

	// API routes
	api := router.Group("/api")
	{
		// Content endpoints
		content := api.Group("/content")
		{
			content.POST("", handlers.CreateContentHandler(hub, dbManager))
			content.GET("/:id", handlers.GetContentHandler())
			content.POST("/:id/vote", handlers.VoteContentHandler(hub, dbManager))
			content.POST("/:id/comment", handlers.CommentContentHandler(hub, dbManager))
			content.POST("/:id/context", handlers.AddContextHandler(hub, dbManager))
			content.GET("/:id/analytics", handlers.GetContentAnalyticsHandler())
		}

		// User endpoints
		user := api.Group("/user")
		{
			user.GET("/:id", handlers.GetUserProfileHandler())
			user.GET("/:id/reputation", handlers.GetUserReputationHandler())
			user.POST("/:id/message", handlers.SendDirectMessageHandler(hub, dbManager))
		}

		// Consensus endpoints
		consensus := api.Group("/consensus")
		{
			consensus.POST("/content/:id", handlers.UpdateConsensusHandler(hub, dbManager))
		}

		// Reputation endpoints
		reputation := api.Group("/reputation")
		{
			reputation.POST("/user/:id", handlers.UpdateReputationHandler(hub, dbManager))
		}

		// Analytics endpoints
		analytics := api.Group("/analytics")
		{
			analytics.GET("/trending", handlers.GetTrendingContentHandler())
		}
	}

	// Start server
	if err := router.Run(":8080"); err != nil {
		log.Fatal("Failed to start server:", err)
	}
}

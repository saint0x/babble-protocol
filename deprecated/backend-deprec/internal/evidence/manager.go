package context

import (
	"fmt"
	"time"

	"github.com/saint/babel-protocol/backend/api/models"
	"github.com/saint/babel-protocol/backend/internal/db/sqlite"
)

// Manager handles context operations
type Manager struct {
	db *sqlite.DBManager
}

// NewManager creates a new context manager
func NewManager(db *sqlite.DBManager) *Manager {
	return &Manager{db: db}
}

// AddContext adds context to a content post
func (m *Manager) AddContext(contentID, authorID string, contextText string, references []string) error {
	// Verify the user is the author
	content, err := m.db.GetContent(contentID)
	if err != nil {
		return fmt.Errorf("failed to get content: %v", err)
	}
	if content.AuthorID != authorID {
		return fmt.Errorf("only the author can add context")
	}

	// Calculate context quality score
	qualityScore := calculateContextQuality(contextText, references)

	context := &models.Evidence{
		ContentID:       contentID,
		SubmitterID:     authorID,
		ContentAuthorID: authorID,
		EvidenceType:    "author_context",
		Text:            contextText,
		References:      references,
		QualityScore:    qualityScore,
		Timestamp:       time.Now(),
		LastUpdated:     time.Now(),
		Metadata: map[string]interface{}{
			"is_author_context": true,
			"display_position":  "top",
		},
	}

	// Store the context
	if err := m.db.CreateEvidence(context); err != nil {
		return fmt.Errorf("failed to create context: %v", err)
	}

	// Update user's context contribution score
	if err := m.updateUserContextScore(authorID); err != nil {
		// Log error but don't fail the operation
		fmt.Printf("failed to update user context score: %v\n", err)
	}

	return nil
}

// calculateContextQuality determines the quality score of context
func calculateContextQuality(text string, references []string) float64 {
	baseScore := 0.6 // Base score for author context

	// Add bonus for substantial text (up to 0.2)
	textBonus := float64(len(text)) / 1000.0 // 1000 chars = full bonus
	if textBonus > 0.2 {
		textBonus = 0.2
	}

	// Add bonus for references (up to 0.2)
	refBonus := float64(len(references)) * 0.05 // 0.05 per reference
	if refBonus > 0.2 {
		refBonus = 0.2
	}

	return baseScore + textBonus + refBonus
}

// updateUserContextScore updates the user's context-related scores
func (m *Manager) updateUserContextScore(userID string) error {
	// Get user's recent context additions
	contexts, err := m.db.GetUserContexts(userID, time.Now().AddDate(0, -1, 0))
	if err != nil {
		return err
	}

	// Calculate average quality and frequency
	var totalQuality float64
	for _, ctx := range contexts {
		totalQuality += ctx.TruthScore
	}

	avgQuality := totalQuality / float64(len(contexts))
	frequency := float64(len(contexts)) / 30.0 // contexts per day

	// Update user's scores
	user, err := m.db.GetUser(userID)
	if err != nil {
		return err
	}

	// Increase engagement quality based on context provision
	engagementBonus := avgQuality * frequency * 0.1
	user.EngagementQuality += engagementBonus
	if user.EngagementQuality > 1.0 {
		user.EngagementQuality = 1.0
	}

	return m.db.UpdateUser(user)
}

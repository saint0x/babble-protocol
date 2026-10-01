package user

import (
	"fmt"
	"time"

	"github.com/saint/babel-protocol/backend/api/models"
	"github.com/saint/babel-protocol/backend/internal/cache"
	"github.com/saint/babel-protocol/backend/internal/db/sqlite"
)

// Manager handles user-related operations and score updates
type Manager struct {
	db    *sqlite.DBManager
	algo  *AlgorithmClient
	cache *cache.Cache
}

// NewManager creates a new user manager
func NewManager(db *sqlite.DBManager, algoURL string) *Manager {
	return &Manager{
		db:    db,
		algo:  NewAlgorithmClient(algoURL),
		cache: cache.NewCache(5 * time.Minute),
	}
}

// UpdateUserScores updates a user's scores based on their recent activity
func (m *Manager) UpdateUserScores(userID string) error {
	// Check cache first
	if _, exists := m.cache.GetUserScores(userID); exists {
		return nil // Scores are fresh enough
	}

	// Get user's recent activity
	user, err := m.db.GetUser(userID)
	if err != nil {
		return fmt.Errorf("failed to get user: %v", err)
	}

	lastUpdate := user.LastActive
	if time.Since(lastUpdate) < time.Minute {
		return nil // Too soon to update
	}

	// Get only new activity since last update
	activities, err := m.db.GetUserActivities(userID, lastUpdate)
	if err != nil {
		return fmt.Errorf("failed to get user activities: %v", err)
	}

	votes, err := m.db.GetUserVotes(userID, lastUpdate)
	if err != nil {
		return fmt.Errorf("failed to get user votes: %v", err)
	}

	evidence, err := m.db.GetUserEvidence(userID, lastUpdate)
	if err != nil {
		return fmt.Errorf("failed to get user evidence: %v", err)
	}

	// Send activity for analysis
	result, err := m.algo.AnalyzeUserActivity(&models.AlgorithmRequest{
		Type:   "user_analysis_incremental",
		UserID: userID,
		Parameters: map[string]interface{}{
			"activities": activities,
			"votes":      votes,
			"evidence":   evidence,
			"current_scores": map[string]float64{
				"truth_accuracy":     user.TruthAccuracy,
				"evidence_quality":   user.EvidenceQuality,
				"engagement_quality": user.EngagementQuality,
				"community_score":    user.CommunityScore,
			},
		},
		Timestamp: time.Now(),
	})

	if err != nil {
		return fmt.Errorf("failed to analyze user activity: %v", err)
	}

	if result.Status != "success" {
		return fmt.Errorf("analysis failed: %v", result.Error)
	}

	// Update user scores
	if scores, ok := result.Results["scores"].(map[string]interface{}); ok {
		user.TruthAccuracy = getFloat64(scores, "truth_accuracy")
		user.EvidenceQuality = getFloat64(scores, "evidence_quality")
		user.EngagementQuality = getFloat64(scores, "engagement_quality")
		user.CommunityScore = getFloat64(scores, "community_score")

		// Calculate overall reputation
		user.ReputationScore = calculateReputationScore(map[string]float64{
			"truth_accuracy":     user.TruthAccuracy,
			"evidence_quality":   user.EvidenceQuality,
			"engagement_quality": user.EngagementQuality,
			"community_score":    user.CommunityScore,
		})

		// Update user in database
		if err := m.db.UpdateUser(user); err != nil {
			return fmt.Errorf("failed to update user: %v", err)
		}

		// Update cache
		m.cache.SetUserScores(userID, map[string]float64{
			"truth_accuracy":     user.TruthAccuracy,
			"evidence_quality":   user.EvidenceQuality,
			"engagement_quality": user.EngagementQuality,
			"community_score":    user.CommunityScore,
			"reputation_score":   user.ReputationScore,
		})
	}

	return nil
}

// UpdateUserAuthenticity updates a user's authenticity score and verification level
func (m *Manager) UpdateUserAuthenticity(userID string) error {
	user, err := m.db.GetUser(userID)
	if err != nil {
		return fmt.Errorf("failed to get user: %v", err)
	}

	// Get verification history
	verifications, err := m.db.GetUserVerifications(userID)
	if err != nil {
		return fmt.Errorf("failed to get verifications: %v", err)
	}

	result, err := m.algo.AnalyzeUserAuthenticity(&models.AlgorithmRequest{
		Type:   "user_authenticity",
		UserID: userID,
		Parameters: map[string]interface{}{
			"public_key": user.PublicKey,
			"activity_history": map[string]interface{}{
				"login_count": user.SessionData.LoginCount,
				"last_ip":     user.SessionData.LastIPAddress,
				"device_info": user.SessionData.DeviceInfo,
			},
			"verifications": verifications,
		},
		Timestamp: time.Now(),
	})

	if err != nil {
		return fmt.Errorf("failed to analyze user authenticity: %v", err)
	}

	if result.Status == "success" {
		if score, ok := result.Results["authenticity_score"].(float64); ok {
			user.AuthenticityScore = score
		}
		if level, ok := result.Results["verification_level"].(float64); ok {
			user.VerificationLevel = int(level)
		}
		if err := m.db.UpdateUser(user); err != nil {
			return fmt.Errorf("failed to update user authenticity: %v", err)
		}
	}

	return nil
}

// Helper functions

func getFloat64(m map[string]interface{}, key string) float64 {
	if val, ok := m[key].(float64); ok {
		return val
	}
	return 0
}

func calculateReputationScore(scores map[string]float64) float64 {
	weights := map[string]float64{
		"truth_accuracy":     0.4,
		"evidence_quality":   0.3,
		"engagement_quality": 0.2,
		"community_score":    0.1,
	}

	var weightedSum, totalWeight float64
	for metric, weight := range weights {
		if score, ok := scores[metric]; ok {
			weightedSum += score * weight
			totalWeight += weight
		}
	}

	if totalWeight == 0 {
		return 0
	}
	return weightedSum / totalWeight
}

// logMetric logs a metric to the database
func (m *Manager) logMetric(name string, value float64, metadata map[string]interface{}) {
	metric := &models.AlgorithmMetric{
		AlgorithmName: "user_manager",
		MetricName:    name,
		Value:         value,
		Timestamp:     time.Now(),
		Metadata:      metadata,
	}
	if err := m.db.LogMetric(metric); err != nil {
		fmt.Printf("Failed to log metric: %v\n", err)
	}
}

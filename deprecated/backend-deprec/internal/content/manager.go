package content

import (
	"fmt"
	"log"
	"math"
	"sync"
	"time"

	"github.com/saint/babel-protocol/backend/api/models"
	"github.com/saint/babel-protocol/backend/internal/cache"
	"github.com/saint/babel-protocol/backend/internal/db/sqlite"
)

// Manager handles content-related operations and integrates with the algorithm service
type Manager struct {
	db    *sqlite.DBManager
	algo  *AlgorithmClient
	cache *cache.Cache

	// Batch processing
	batchMu     sync.Mutex
	batchBuffer map[string]*models.Content
	batchTimer  *time.Timer

	// Score caching
	scoreMu    sync.RWMutex
	scoreCache map[string]*scoreInfo
}

type scoreInfo struct {
	truthScore      float64
	visibilityScore float64
	lastCalculated  time.Time
	expiresAt       time.Time
}

// NewManager creates a new content manager instance
func NewManager(db *sqlite.DBManager, algoURL string) *Manager {
	m := &Manager{
		db:          db,
		algo:        NewAlgorithmClient(algoURL),
		cache:       cache.NewCache(5 * time.Minute),
		batchBuffer: make(map[string]*models.Content),
		scoreCache:  make(map[string]*scoreInfo),
	}

	// Start batch processor
	m.startBatchProcessor()

	// Start cache cleanup
	go m.startCacheCleanup()
	return m
}

// CreateContent initializes new content with default values and stores it
func (m *Manager) CreateContent(content *models.Content) error {
	// Set timestamps
	now := time.Now()
	content.Timestamp = now
	content.LastUpdated = now
	content.ProcessingStatus = "pending"

	// Store in database
	if err := m.db.CreateContent(content); err != nil {
		return fmt.Errorf("failed to create content: %v", err)
	}

	// Add to batch processing queue
	m.queueForProcessing(content)

	return nil
}

// GetContent retrieves content by ID and enriches it with additional data
func (m *Manager) GetContent(id string) (*models.Content, error) {
	// Check cache first
	if scores, exists := m.cache.GetContentScores(id); exists {
		content, err := m.db.GetContent(id)
		if err != nil {
			return nil, fmt.Errorf("failed to get content: %v", err)
		}
		content.TruthScore = scores.TruthScore
		content.VisibilityScore = scores.VisibilityScore
		return content, nil
	}

	// Cache miss, get full content
	content, err := m.db.GetContent(id)
	if err != nil {
		return nil, fmt.Errorf("failed to get content: %v", err)
	}
	if content == nil {
		return nil, nil
	}

	// Get votes and convert to models.Vote
	voteInfos, err := m.db.GetContentVotes(id)
	if err != nil {
		return nil, fmt.Errorf("failed to get content votes: %v", err)
	}

	votes := make([]*models.Vote, len(voteInfos))
	for i, info := range voteInfos {
		votes[i] = &models.Vote{
			ContentID:   info.ContentID,
			UserID:      info.VoterID,
			Type:        info.VoteType,
			Weight:      info.VoteWeight,
			EvidenceIDs: info.EvidenceIDs,
			Timestamp:   info.Timestamp,
			LastUpdated: info.LastUpdated,
		}
	}

	// Calculate scores
	content.TruthScore = calculateTruthScore(votes)
	content.VisibilityScore = calculateVisibilityScore(votes)

	// Update cache
	m.cache.SetContentScores(id, content.TruthScore, content.VisibilityScore)

	return content, nil
}

// queueForProcessing adds content to the batch processing queue
func (m *Manager) queueForProcessing(content *models.Content) {
	m.batchMu.Lock()
	defer m.batchMu.Unlock()

	m.batchBuffer[content.ID] = content

	// Reset batch timer
	if m.batchTimer != nil {
		m.batchTimer.Reset(time.Second * 5)
	}
}

// startBatchProcessor starts the background batch processor
func (m *Manager) startBatchProcessor() {
	m.batchTimer = time.NewTimer(time.Second * 5)
	go func() {
		for range m.batchTimer.C {
			m.processBatch()
		}
	}()
}

// processBatch processes all queued content
func (m *Manager) processBatch() {
	m.batchMu.Lock()
	if len(m.batchBuffer) == 0 {
		m.batchMu.Unlock()
		return
	}

	// Get batch and clear buffer
	batch := m.batchBuffer
	m.batchBuffer = make(map[string]*models.Content)
	m.batchMu.Unlock()

	// Process batch
	contentList := make([]*models.Content, 0, len(batch))
	consensusRequests := make([]map[string]interface{}, 0, len(batch))

	// Prepare all requests in parallel
	var wg sync.WaitGroup
	requestChan := make(chan struct {
		content *models.Content
		request map[string]interface{}
	}, len(batch))

	for _, content := range batch {
		wg.Add(1)
		go func(c *models.Content) {
			defer wg.Done()

			voteInfos, err := m.db.GetContentVotes(c.ID)
			if err != nil {
				m.logError("GetContentVotes", fmt.Errorf("failed to get votes for content %s: %v", c.ID, err))
				return
			}

			sources := make([]map[string]interface{}, len(voteInfos))
			for j, vote := range voteInfos {
				sources[j] = map[string]interface{}{
					"user_id":    vote.VoterID,
					"vote_value": getVoteTypeValue(vote.VoteType, vote.CertaintyLevel),
					"timestamp":  vote.Timestamp.Unix(),
					"weight":     vote.VoteWeight,
				}
			}

			requestChan <- struct {
				content *models.Content
				request map[string]interface{}
			}{
				content: c,
				request: map[string]interface{}{
					"content_id":               c.ID,
					"sources":                  sources,
					"previous_consensus_score": c.Consensus.Score,
				},
			}
		}(content)
	}

	// Close channel when all goroutines are done
	go func() {
		wg.Wait()
		close(requestChan)
	}()

	// Collect results
	for req := range requestChan {
		contentList = append(contentList, req.content)
		consensusRequests = append(consensusRequests, req.request)
	}

	// Send batch requests
	contentResult, err := m.algo.AnalyzeContentBatch(&models.AlgorithmRequest{
		Type: "content_analysis_batch",
		Parameters: map[string]interface{}{
			"content_batch": contentList,
		},
		Timestamp: time.Now(),
	})

	if err != nil {
		m.logError("ProcessBatch", err)
		return
	}

	// Process results and update cache in parallel
	var updateWg sync.WaitGroup
	for _, content := range contentList {
		updateWg.Add(1)
		go func(c *models.Content) {
			defer updateWg.Done()

			if analysis, ok := contentResult.Results[c.ID].(*models.ContentAnalysis); ok {
				c.Topics = analysis.Topics
				c.Entities = analysis.Entities
				c.TruthScore = analysis.TruthScore

				// Update score cache
				m.scoreMu.Lock()
				m.scoreCache[c.ID] = &scoreInfo{
					truthScore:      c.TruthScore,
					visibilityScore: c.VisibilityScore,
					lastCalculated:  time.Now(),
					expiresAt:       time.Now().Add(15 * time.Minute),
				}
				m.scoreMu.Unlock()
			}
		}(content)
	}

	// Wait for all updates to complete
	updateWg.Wait()

	// Batch update to database
	if err := m.db.UpdateContentBatch(contentList); err != nil {
		m.logError("UpdateContentBatch", fmt.Errorf("failed to update content batch: %v", err))
		return
	}

	// Process each content item for context-specific updates
	now := time.Now()
	for _, c := range contentList {
		c.LastUpdated = now
		if c.ContentType == "context" {
			// Apply temporal decay to context impact
			age := time.Since(c.Timestamp)
			decayFactor := math.Exp(-age.Hours() / (30 * 24)) // 30-day half-life
			contextImpact := c.TruthScore * decayFactor

			// Update author's engagement quality
			if author, err := m.db.GetUser(c.AuthorID); err == nil {
				author.EngagementQuality += contextImpact * 0.1
				author.LastActive = now
				if author.EngagementQuality > 1.0 {
					author.EngagementQuality = 1.0
				}
				m.db.UpdateUser(author)
			}
		}
	}
}

// ValidateContent checks if the content is valid
func (m *Manager) ValidateContent(content *models.Content) error {
	if content.ID == "" {
		return fmt.Errorf("content ID is required")
	}
	if content.AuthorID == "" {
		return fmt.Errorf("author ID is required")
	}
	if content.ContentText == "" {
		return fmt.Errorf("content text is required")
	}
	if content.ContentType == "" {
		return fmt.Errorf("content type is required")
	}
	return nil
}

// Helper functions

func calculateTruthScore(votes []*models.Vote) float64 {
	if len(votes) == 0 {
		return 0.0
	}

	var weightedSum, totalWeight float64

	// Use exponential moving average for O(1) updates
	for _, vote := range votes {
		if vote.Type == models.VoteTypeAffirm || vote.Type == models.VoteTypeDeny {
			weight := vote.Weight * math.Exp(-time.Since(vote.Timestamp).Hours()/24.0)
			voteValue := getVoteTypeValue(vote.Type, vote.CertaintyLevel)
			weightedSum += weight * voteValue
			totalWeight += weight
		}
	}

	if totalWeight == 0 {
		return 0.0
	}
	return weightedSum / totalWeight
}

func calculateVisibilityScore(votes []*models.Vote) float64 {
	if len(votes) == 0 {
		return 1.0 // Default visibility
	}

	const (
		decayHalfLife = 24.0 * time.Hour
		minVisibility = 0.1
		maxVisibility = 1.0
	)

	var engagementSum, totalWeight float64

	for _, vote := range votes {
		// Calculate time decay factor
		ageFactor := math.Exp(-time.Since(vote.Timestamp).Hours() / decayHalfLife.Hours())
		weight := vote.Weight * ageFactor

		// Weight different vote types
		switch vote.Type {
		case models.VoteTypeUpvote, models.VoteTypeDownvote:
			engagementSum += weight * 1.0
		case models.VoteTypeAffirm, models.VoteTypeDeny:
			engagementSum += weight * 1.2
		case models.VoteTypeEngage:
			engagementSum += weight * 1.5
		case models.VoteTypeUnengage:
			engagementSum += weight * 0.5
		}
		totalWeight += weight
	}

	if totalWeight == 0 {
		return minVisibility
	}

	// Normalize score
	score := (engagementSum / totalWeight) / 1.5
	return math.Max(minVisibility, math.Min(maxVisibility, score))
}

func getVoteTypeValue(voteType string, certaintyLevel int) float64 {
	switch voteType {
	case models.VoteTypeUpvote:
		return 1.0
	case models.VoteTypeDownvote:
		return 0.0
	case models.VoteTypeAffirm:
		// Scale affirmation by certainty level (1-3)
		return 0.5 + float64(certaintyLevel)*0.25 // Results in 0.75, 1.0, or 1.25
	case models.VoteTypeDeny:
		// Scale denial by certainty level (1-3)
		return 0.5 - float64(certaintyLevel)*0.25 // Results in 0.25, 0.0, or -0.25
	case models.VoteTypeEngage:
		return 0.75 // Positive but less than a full upvote
	case models.VoteTypeUnengage:
		return 0.25 // Negative but less than a full downvote
	default:
		return 0.5 // Neutral for unknown vote types
	}
}

// logError logs an error to the database with precise timestamp
func (m *Manager) logError(operation string, err error) {
	if err := m.db.LogError(&models.AlgorithmError{
		AlgorithmName: "content_manager",
		ErrorType:     operation,
		ErrorMessage:  err.Error(),
		Timestamp:     time.Now(),
	}); err != nil {
		log.Printf("failed to log error: %v", err)
	}
}

// logMetric logs a metric to the database with precise timestamp
func (m *Manager) logMetric(name string, value float64, metadata map[string]interface{}) {
	if err := m.db.LogMetric(&models.AlgorithmMetric{
		AlgorithmName: "content_manager",
		MetricName:    name,
		Value:         value,
		Timestamp:     time.Now(),
		Metadata:      metadata,
	}); err != nil {
		log.Printf("failed to log metric: %v", err)
	}
}

func getConsensusState(score float64) string {
	switch {
	case score >= 0.8:
		return "established"
	case score >= 0.6:
		return "provisional"
	case score >= 0.4:
		return "emerging"
	default:
		return "insufficient"
	}
}

// startCacheCleanup periodically removes expired scores
func (m *Manager) startCacheCleanup() {
	ticker := time.NewTicker(5 * time.Minute)
	for range ticker.C {
		m.scoreMu.Lock()
		now := time.Now()
		for id, info := range m.scoreCache {
			if now.After(info.expiresAt) {
				delete(m.scoreCache, id)
			}
		}
		m.scoreMu.Unlock()
	}
}

// AddContext adds context to an existing content post
func (m *Manager) AddContext(parentID string, context *models.Content) error {
	// Verify the user is the author
	parent, err := m.db.GetContent(parentID)
	if err != nil {
		return fmt.Errorf("failed to get parent content: %v", err)
	}
	if parent.AuthorID != context.AuthorID {
		return fmt.Errorf("only the author can add context")
	}

	// Set up context content with timestamps
	now := time.Now()
	context.ContentType = "context"
	context.ParentID = &parentID
	context.ProcessingStatus = "pending"
	context.Timestamp = now
	context.LastUpdated = now

	// Calculate initial scores
	contextScore := calculateContextScore(context)
	context.TruthScore = contextScore
	context.VisibilityScore = 1.0 // Context is always visible

	// Store the context
	if err := m.db.CreateContent(context); err != nil {
		return fmt.Errorf("failed to create context: %v", err)
	}

	// Queue for processing
	m.queueForProcessing(context)

	// Update parent content's scores with timestamp
	parent.TruthScore = updateTruthScoreWithContext(parent.TruthScore, contextScore)
	parent.LastUpdated = now
	if err := m.db.UpdateContent(parent); err != nil {
		// Log error but don't fail the operation
		fmt.Printf("failed to update parent content: %v\n", err)
	}

	// Update user's context contribution score
	if err := m.updateUserContextScore(context.AuthorID); err != nil {
		// Log error but don't fail the operation
		fmt.Printf("failed to update user context score: %v\n", err)
	}

	return nil
}

// calculateContextScore determines the quality score of context
func calculateContextScore(content *models.Content) float64 {
	baseScore := 0.6 // Base score for context

	// Add bonus for substantial text (up to 0.2)
	textBonus := float64(len(content.ContentText)) / 1000.0
	if textBonus > 0.2 {
		textBonus = 0.2
	}

	// Add bonus for media and references (up to 0.2)
	mediaBonus := float64(len(content.MediaURLs)) * 0.05
	if mediaBonus > 0.2 {
		mediaBonus = 0.2
	}

	return baseScore + textBonus + mediaBonus
}

// updateTruthScoreWithContext updates a content's truth score based on context
func updateTruthScoreWithContext(currentScore, contextScore float64) float64 {
	// Context can improve score by up to 20%
	improvement := contextScore * 0.2
	return math.Min(1.0, currentScore+improvement)
}

// updateUserContextScore updates a user's context contribution score
func (m *Manager) updateUserContextScore(userID string) error {
	// Get user's recent context posts
	contexts, err := m.db.GetUserContexts(userID, time.Now().AddDate(0, -1, 0))
	if err != nil {
		return fmt.Errorf("failed to get user contexts: %v", err)
	}

	// Calculate average context score with temporal decay
	var weightedSum, totalWeight float64
	now := time.Now()

	for _, ctx := range contexts {
		age := now.Sub(ctx.Timestamp)
		weight := math.Exp(-age.Hours() / (30 * 24)) // 30-day half-life
		weightedSum += ctx.TruthScore * weight
		totalWeight += weight
	}

	// Update user's context contribution score
	if totalWeight > 0 {
		score := weightedSum / totalWeight
		if user, err := m.db.GetUser(userID); err == nil {
			user.ContextQuality = score
			return m.db.UpdateUser(user)
		}
	}

	return nil
}

package models

import (
	"encoding/json"
	"fmt"
	"time"
)

// Content represents a piece of content in the system
type Content struct {
	ID               string                 `json:"id"`
	AuthorID         string                 `json:"author_id"`
	ContentType      string                 `json:"content_type"`
	ContentText      string                 `json:"content_text"`
	MediaURLs        []string               `json:"media_urls"`
	ParentID         *string                `json:"parent_id,omitempty"`
	IsContext        bool                   `json:"is_context"`
	Timestamp        time.Time              `json:"timestamp"`
	Signature        string                 `json:"signature"`
	Hash             string                 `json:"hash"`
	ProcessingStatus string                 `json:"processing_status"`
	LastUpdated      time.Time              `json:"last_updated"`
	Metadata         map[string]interface{} `json:"metadata,omitempty"`
	TruthScore       float64                `json:"truth_score"`
	VisibilityScore  float64                `json:"visibility_score"`
	EvidenceChains   []string               `json:"evidence_chains"`
	Topics           []string               `json:"topics"`
	Entities         []string               `json:"entities"`
	ContextRefs      []string               `json:"context_refs"`
	Consensus        ConsensusInfo          `json:"consensus"`
}

// Vote types
const (
	VoteTypeUpvote   = "upvote"
	VoteTypeDownvote = "downvote"
	VoteTypeAffirm   = "affirm"
	VoteTypeDeny     = "deny"
	VoteTypeEngage   = "engage"
	VoteTypeUnengage = "unengage"
)

// Vote represents a vote on a piece of content
type Vote struct {
	ID             string    `json:"id"`
	ContentID      string    `json:"content_id"`
	UserID         string    `json:"user_id"`
	Type           string    `json:"type"`            // upvote/downvote/affirm/deny/engage/unengage
	Weight         float64   `json:"weight"`          // Base weight of the vote
	CertaintyLevel int       `json:"certainty_level"` // 1-3 for affirm/deny votes
	EvidenceIDs    []string  `json:"evidence_ids"`
	Timestamp      time.Time `json:"timestamp"`
	LastUpdated    time.Time `json:"last_updated"`
	Explanation    string    `json:"explanation,omitempty"`
	ContextScore   float64   `json:"context_score"`
}

// Helper function to validate vote type and certainty level
func (v *Vote) Validate() error {
	switch v.Type {
	case VoteTypeUpvote, VoteTypeDownvote:
		if v.CertaintyLevel != 0 {
			return fmt.Errorf("certainty level should not be set for upvote/downvote")
		}
	case VoteTypeAffirm, VoteTypeDeny:
		if v.CertaintyLevel < 1 || v.CertaintyLevel > 3 {
			return fmt.Errorf("certainty level must be between 1 and 3 for affirm/deny votes")
		}
	case VoteTypeEngage, VoteTypeUnengage:
		if v.CertaintyLevel != 0 {
			return fmt.Errorf("certainty level should not be set for engage/unengage")
		}
	default:
		return fmt.Errorf("invalid vote type: %s", v.Type)
	}
	return nil
}

// Evidence represents supporting evidence or context for content
type Evidence struct {
	ID                string                 `json:"id"`
	ContentID         string                 `json:"content_id"`
	SubmitterID       string                 `json:"submitter_id"`
	ContentAuthorID   string                 `json:"content_author_id"`
	EvidenceType      string                 `json:"evidence_type"`
	URL               string                 `json:"url,omitempty"`
	Text              string                 `json:"text,omitempty"`
	MediaHash         string                 `json:"media_hash,omitempty"`
	Description       string                 `json:"description,omitempty"`
	EvidenceText      string                 `json:"evidence_text"`
	References        []string               `json:"references"`
	QualityScore      float64                `json:"quality_score"`
	ContextScore      float64                `json:"context_score"`
	VerificationState string                 `json:"verification_state"`
	ContextData       map[string]interface{} `json:"context_data,omitempty"`
	Timestamp         time.Time              `json:"timestamp"`
	LastUpdated       time.Time              `json:"last_updated"`
	Metadata          map[string]interface{} `json:"metadata"`
}

// EvidenceInteraction represents user interactions with evidence
type EvidenceInteraction struct {
	ID          string    `json:"id"`
	EvidenceID  string    `json:"evidence_id"`
	UserID      string    `json:"user_id"`
	Type        string    `json:"type"` // helpful, not_helpful
	Timestamp   time.Time `json:"timestamp"`
	LastUpdated time.Time `json:"last_updated"`
}

// User represents a user in the system
type User struct {
	ID                 string                   `json:"id"`
	PublicKey          string                   `json:"public_key"`
	Username           string                   `json:"username"`
	CreatedAt          time.Time                `json:"created_at"`
	AuthenticityScore  float64                  `json:"authenticity_score"`
	ReputationScore    float64                  `json:"reputation_score"`
	TruthAccuracy      float64                  `json:"truth_accuracy"`
	EvidenceQuality    float64                  `json:"evidence_quality"`
	EngagementQuality  float64                  `json:"engagement_quality"`
	CommunityScore     float64                  `json:"community_score"`
	LastActive         time.Time                `json:"last_active"`
	SessionData        *UserSession             `json:"session_data,omitempty"`
	Preferences        *UserPreferences         `json:"preferences,omitempty"`
	StakeAmount        float64                  `json:"stake_amount"`
	StakeLockedUntil   *time.Time               `json:"stake_locked_until,omitempty"`
	DomainExpertise    map[string]ExpertiseInfo `json:"domain_expertise,omitempty"`
	VerificationLevel  int                      `json:"verification_level"`
	TotalContributions int                      `json:"total_contributions"`
	Achievements       []*Achievement           `json:"achievements,omitempty"`
	Relationships      []*UserRelationship      `json:"relationships,omitempty"`
	ContextQuality     float64                  `json:"context_quality"` // Quality of context contributions
}

// ExpertiseInfo represents domain expertise information
type ExpertiseInfo struct {
	ExpertiseScore     float64   `json:"expertise_score"`
	ConfidenceScore    float64   `json:"confidence_score"`
	LastUpdated        time.Time `json:"last_updated"`
	VerificationProofs []string  `json:"verification_proofs,omitempty"`
}

// UserRelationship represents a relationship between users
type UserRelationship struct {
	FollowerID       string                 `json:"follower_id"`
	FollowingID      string                 `json:"following_id"`
	RelationType     string                 `json:"relationship_type"` // follow/trust/collaborate
	TrustScore       float64                `json:"trust_score"`
	InteractionCount int                    `json:"interaction_count"`
	LastInteraction  time.Time              `json:"last_interaction"`
	Metadata         map[string]interface{} `json:"metadata,omitempty"`
}

// Achievement represents a user achievement
type Achievement struct {
	ID              string                 `json:"id"`
	UserID          string                 `json:"user_id"`
	AchievementType string                 `json:"achievement_type"`
	EarnedAt        time.Time              `json:"earned_at"`
	Level           int                    `json:"level"`
	Progress        float64                `json:"progress"`
	Metadata        map[string]interface{} `json:"metadata,omitempty"`
}

// UserVerification represents a verification record
type UserVerification struct {
	ID               string                 `json:"id"`
	UserID           string                 `json:"user_id"`
	VerificationType string                 `json:"verification_type"`
	Status           string                 `json:"status"`
	VerifiedAt       *time.Time             `json:"verified_at,omitempty"`
	VerifierID       string                 `json:"verifier_id,omitempty"`
	ProofData        map[string]interface{} `json:"proof_data,omitempty"`
	Metadata         map[string]interface{} `json:"metadata,omitempty"`
}

// UserActivity represents a user activity record
type UserActivity struct {
	ID           string                 `json:"id"`
	UserID       string                 `json:"user_id"`
	ActivityType string                 `json:"activity_type"`
	TargetID     string                 `json:"target_id,omitempty"`
	Timestamp    time.Time              `json:"timestamp"`
	ImpactScore  float64                `json:"impact_score"`
	Metadata     map[string]interface{} `json:"metadata,omitempty"`
}

// UserSession represents user session data
type UserSession struct {
	LastLogin     time.Time `json:"last_login"`
	LoginCount    int       `json:"login_count"`
	LastIPAddress string    `json:"last_ip_address"`
	DeviceInfo    string    `json:"device_info"`
	SessionToken  string    `json:"session_token"`
}

// UserPreferences represents user preferences
type UserPreferences struct {
	ContentFilters    []string           `json:"content_filters"`
	TopicInterests    map[string]float64 `json:"topic_interests"`
	PrivacySettings   PrivacySettings    `json:"privacy_settings"`
	NotificationPrefs NotificationPrefs  `json:"notification_prefs"`
}

// PrivacySettings represents user privacy settings
type PrivacySettings struct {
	IsPublic         bool `json:"is_public"`
	ShowVotes        bool `json:"show_votes"`
	ShowEvidence     bool `json:"show_evidence"`
	AllowAnonymous   bool `json:"allow_anonymous"`
	EnableEncryption bool `json:"enable_encryption"`
}

// NotificationPrefs represents user notification preferences
type NotificationPrefs struct {
	EnableEmail      bool `json:"enable_email"`
	EnablePush       bool `json:"enable_push"`
	ContentUpdates   bool `json:"content_updates"`
	VoteAlerts       bool `json:"vote_alerts"`
	EvidenceAlerts   bool `json:"evidence_alerts"`
	ReputationAlerts bool `json:"reputation_alerts"`
}

// FeedRequest represents a request for content feed
type FeedRequest struct {
	UserID      string                 `json:"user_id"`
	PageSize    int                    `json:"page_size"`
	LastID      string                 `json:"last_id,omitempty"`
	Filters     map[string]interface{} `json:"filters,omitempty"`
	SortBy      string                 `json:"sort_by"`
	ContextData map[string]interface{} `json:"context_data,omitempty"`
}

// FeedResponse represents a response containing feed content
type FeedResponse struct {
	Items       []Content `json:"items"`
	NextID      string    `json:"next_id,omitempty"`
	HasMore     bool      `json:"has_more"`
	TotalCount  int       `json:"total_count"`
	GeneratedAt time.Time `json:"generated_at"`
}

// AlgorithmRequest represents a request to the algorithm service
type AlgorithmRequest struct {
	Type       string                 `json:"type"`
	ContentID  string                 `json:"content_id,omitempty"`
	UserID     string                 `json:"user_id,omitempty"`
	Parameters map[string]interface{} `json:"parameters,omitempty"`
	Timestamp  time.Time              `json:"timestamp"`
}

// AlgorithmResponse represents a response from the algorithm service
type AlgorithmResponse struct {
	Type      string                 `json:"type"`
	Results   map[string]interface{} `json:"results"`
	Metrics   map[string]float64     `json:"metrics,omitempty"`
	Timestamp time.Time              `json:"timestamp"`
	Status    string                 `json:"status"`          // success/failed/partial
	Error     *AlgorithmError        `json:"error,omitempty"` // Error details if status is failed
	// Specific result fields for type safety
	ContentAnalysis  *ContentAnalysis  `json:"content_analysis,omitempty"`
	EvidenceAnalysis *EvidenceAnalysis `json:"evidence_analysis,omitempty"`
	ConsensusResult  *ConsensusResult  `json:"consensus_result,omitempty"`
}

// ContentAnalysis represents the results of content analysis
type ContentAnalysis struct {
	Topics         []string               `json:"topics"`
	Entities       []string               `json:"entities"`
	TruthScore     float64                `json:"truth_score"`
	ContextScore   float64                `json:"context_score"`
	Sentiment      map[string]float64     `json:"sentiment"`
	Classification map[string]float64     `json:"classification"`
	KeyPhrases     []string               `json:"key_phrases"`
	RelatedContent []string               `json:"related_content"`
	Metadata       map[string]interface{} `json:"metadata,omitempty"`
}

// EvidenceAnalysis represents the results of evidence analysis
type EvidenceAnalysis struct {
	VerificationState string                 `json:"verification_state"`
	QualityScore      float64                `json:"quality_score"`
	RelevanceScore    float64                `json:"relevance_score"`
	SourceReputation  float64                `json:"source_reputation"`
	CrossReferences   []string               `json:"cross_references"`
	FactChecks        []FactCheck            `json:"fact_checks"`
	Metadata          map[string]interface{} `json:"metadata,omitempty"`
}

// ConsensusResult represents the results of consensus processing
type ConsensusResult struct {
	TruthConsensus    float64                `json:"truth_consensus"`
	ConfidenceScore   float64                `json:"confidence_score"`
	ParticipantCount  int                    `json:"participant_count"`
	VoteDistribution  map[string]int         `json:"vote_distribution"`
	StakeholderStats  map[string]float64     `json:"stakeholder_stats"`
	ConsensusReached  bool                   `json:"consensus_reached"`
	ConsensusMetadata map[string]interface{} `json:"consensus_metadata,omitempty"`
}

// FactCheck represents a fact-checking result
type FactCheck struct {
	Claim       string    `json:"claim"`
	Verdict     string    `json:"verdict"`
	Explanation string    `json:"explanation"`
	Sources     []string  `json:"sources"`
	Timestamp   time.Time `json:"timestamp"`
}

// Helper methods for JSON handling

func (c *Content) MarshalMediaURLs() (string, error) {
	if len(c.MediaURLs) == 0 {
		return "[]", nil
	}
	data, err := json.Marshal(c.MediaURLs)
	if err != nil {
		return "", err
	}
	return string(data), nil
}

func (c *Content) UnmarshalMediaURLs(data string) error {
	if data == "" || data == "[]" {
		c.MediaURLs = []string{}
		return nil
	}
	return json.Unmarshal([]byte(data), &c.MediaURLs)
}

func (u *User) MarshalSessionData() (string, error) {
	if u.SessionData == nil {
		return "", nil
	}
	data, err := json.Marshal(u.SessionData)
	if err != nil {
		return "", err
	}
	return string(data), nil
}

func (u *User) UnmarshalSessionData(data string) error {
	if data == "" {
		u.SessionData = nil
		return nil
	}
	u.SessionData = &UserSession{}
	return json.Unmarshal([]byte(data), u.SessionData)
}

// AlgorithmError represents an error that occurred during algorithm execution
type AlgorithmError struct {
	AlgorithmName   string                 `json:"algorithm_name"`
	ErrorType       string                 `json:"error_type"`
	ErrorMessage    string                 `json:"error_message"`
	Context         map[string]interface{} `json:"context"`
	Timestamp       time.Time              `json:"timestamp"`
	Resolved        bool                   `json:"resolved"`
	ResolutionNotes string                 `json:"resolution_notes,omitempty"`
}

// AlgorithmMetric represents a metric recorded during algorithm execution
type AlgorithmMetric struct {
	AlgorithmName string                 `json:"algorithm_name"`
	MetricName    string                 `json:"metric_name"`
	Value         float64                `json:"value"`
	Timestamp     time.Time              `json:"timestamp"`
	Metadata      map[string]interface{} `json:"metadata,omitempty"`
}

type ConsensusInfo struct {
	State             string             `json:"state"`
	Score             float64            `json:"score"`
	LastUpdated       time.Time          `json:"last_updated"`
	ValidatorCount    int                `json:"validator_count"`
	PreviousScore     float64            `json:"previous_score,omitempty"`
	UserContributions map[string]float64 `json:"user_contributions,omitempty"`
	TemporalWeight    float64            `json:"temporal_weight"`
}

// Comment represents a comment on content
type Comment struct {
	ID        string    `json:"id"`
	ContentID string    `json:"content_id"`
	AuthorID  string    `json:"author_id"`
	Text      string    `json:"text"`
	ParentID  string    `json:"parent_id,omitempty"` // For nested comments
	Timestamp time.Time `json:"timestamp"`
}

// DirectMessage represents a private message between users
type DirectMessage struct {
	ID         string    `json:"id"`
	SenderID   string    `json:"sender_id"`
	ReceiverID string    `json:"receiver_id"`
	Text       string    `json:"text"`
	Timestamp  time.Time `json:"timestamp"`
	ReadAt     time.Time `json:"read_at,omitempty"`
}

// ConsensusUpdate represents a change in consensus state
type ConsensusUpdate struct {
	ContentID    string    `json:"content_id"`
	State        string    `json:"state"` // FORMING, REACHED, CHALLENGED
	Score        float64   `json:"score"` // Current consensus score
	Participants int       `json:"participants"`
	Timestamp    time.Time `json:"timestamp"`
}

// ReputationUpdate represents a change in user reputation
type ReputationUpdate struct {
	UserID     string    `json:"user_id"`
	OldScore   float64   `json:"old_score"`
	NewScore   float64   `json:"new_score"`
	Reason     string    `json:"reason"`
	ChangeType string    `json:"change_type"` // VOTE_ACCURACY, EVIDENCE_QUALITY, etc.
	Timestamp  time.Time `json:"timestamp"`
}

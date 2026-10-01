package sqlite

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	"github.com/saint/babel-protocol/backend/api/models"
)

// Content Operations

func (m *DBManager) CreateContent(content *models.Content) error {
	mediaURLs, err := content.MarshalMediaURLs()
	if err != nil {
		return fmt.Errorf("failed to marshal media URLs: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT INTO content (
				id, author_id, content_type, content_text, media_urls,
				parent_id, timestamp, signature, hash, processing_status, last_updated
			) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			content.ID, content.AuthorID, content.ContentType, content.ContentText, mediaURLs,
			content.ParentID, content.Timestamp.Unix(), content.Signature, content.Hash,
			content.ProcessingStatus, content.LastUpdated.Unix(),
		)
		return err
	})
}

func (m *DBManager) GetContent(id string) (*models.Content, error) {
	var content models.Content
	var timestamp, lastUpdated int64
	var mediaURLs string

	err := m.db.QueryRow(`
		SELECT id, author_id, content_type, content_text, media_urls,
			   parent_id, timestamp, signature, hash, processing_status, last_updated
		FROM content WHERE id = ?`, id).Scan(
		&content.ID, &content.AuthorID, &content.ContentType, &content.ContentText, &mediaURLs,
		&content.ParentID, &timestamp, &content.Signature, &content.Hash,
		&content.ProcessingStatus, &lastUpdated,
	)
	if err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}

	content.Timestamp = time.Unix(timestamp, 0)
	content.LastUpdated = time.Unix(lastUpdated, 0)
	if err := content.UnmarshalMediaURLs(mediaURLs); err != nil {
		return nil, err
	}

	return &content, nil
}

func (m *DBManager) UpdateContent(content *models.Content) error {
	mediaURLs, err := content.MarshalMediaURLs()
	if err != nil {
		return fmt.Errorf("failed to marshal media URLs: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		result, err := tx.Exec(`
			UPDATE content SET
				content_type = ?, content_text = ?, media_urls = ?,
				parent_id = ?, signature = ?, hash = ?,
				processing_status = ?, last_updated = ?
			WHERE id = ? AND author_id = ?`,
			content.ContentType, content.ContentText, mediaURLs,
			content.ParentID, content.Signature, content.Hash,
			content.ProcessingStatus, time.Now().Unix(),
			content.ID, content.AuthorID,
		)
		if err != nil {
			return err
		}

		rows, err := result.RowsAffected()
		if err != nil {
			return err
		}
		if rows == 0 {
			return fmt.Errorf("content not found or user not authorized")
		}

		return nil
	})
}

// User Operations

func (m *DBManager) CreateUser(user *models.User) error {
	sessionData, err := user.MarshalSessionData()
	if err != nil {
		return fmt.Errorf("failed to marshal session data: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT INTO users (
				id, public_key, username, created_at,
				authenticity_score, reputation_score, last_active, session_data
			) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
			user.ID, user.PublicKey, user.Username, user.CreatedAt.Unix(),
			user.AuthenticityScore, user.ReputationScore, user.LastActive.Unix(), sessionData,
		)
		return err
	})
}

func (m *DBManager) GetUser(id string) (*models.User, error) {
	var user models.User
	var createdAt, lastActive int64
	var sessionData string

	err := m.db.QueryRow(`
		SELECT id, public_key, username, created_at,
			   authenticity_score, reputation_score, last_active, session_data
		FROM users WHERE id = ?`, id).Scan(
		&user.ID, &user.PublicKey, &user.Username, &createdAt,
		&user.AuthenticityScore, &user.ReputationScore, &lastActive, &sessionData,
	)
	if err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}

	user.CreatedAt = time.Unix(createdAt, 0)
	user.LastActive = time.Unix(lastActive, 0)
	if err := user.UnmarshalSessionData(sessionData); err != nil {
		return nil, err
	}

	return &user, nil
}

// Algorithm Cache Operations

func (m *DBManager) SetCache(key string, value string, expiry time.Time) error {
	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT OR REPLACE INTO algorithm_cache (
				key, value, expiry, created_at, last_accessed
			) VALUES (?, ?, ?, ?, ?)`,
			key, value, expiry.Unix(),
			time.Now().Unix(),
			sql.NullInt64{
				Int64: time.Now().Unix(),
				Valid: true,
			},
		)
		return err
	})
}

func (m *DBManager) GetCache(key string) (string, error) {
	var value string
	var expiry int64

	err := m.db.QueryRow(`
		SELECT value, expiry
		FROM algorithm_cache WHERE key = ? AND expiry > ?`,
		key, time.Now().Unix(),
	).Scan(&value, &expiry)

	if err != nil {
		if err == sql.ErrNoRows {
			return "", nil
		}
		return "", err
	}

	return value, nil
}

// Consensus Operations

func (m *DBManager) RecordVote(contentID, voterID, voteType string, voteWeight float64, certaintyLevel int, evidenceIDs []string) error {
	evidenceIDsJSON, err := json.Marshal(evidenceIDs)
	if err != nil {
		return fmt.Errorf("failed to marshal evidence IDs: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT OR REPLACE INTO truth_consensus (
				content_id, voter_id, vote_type, vote_weight,
				certainty_level, evidence_ids, timestamp, last_updated
			) VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
			contentID, voterID, voteType, voteWeight,
			certaintyLevel, string(evidenceIDsJSON), time.Now().Unix(), time.Now().Unix(),
		)
		return err
	})
}

type VoteInfo struct {
	ContentID      string    `json:"content_id"`
	VoterID        string    `json:"voter_id"`
	VoteType       string    `json:"vote_type"`
	VoteWeight     float64   `json:"vote_weight"`
	CertaintyLevel int       `json:"certainty_level"`
	EvidenceIDs    []string  `json:"evidence_ids"`
	Timestamp      time.Time `json:"timestamp"`
	LastUpdated    time.Time `json:"last_updated"`
}

func (m *DBManager) GetContentVotes(contentID string) ([]*VoteInfo, error) {
	rows, err := m.db.Query(`
		SELECT content_id, voter_id, vote_type, vote_weight,
			   certainty_level, evidence_ids, timestamp, last_updated
		FROM truth_consensus WHERE content_id = ?`, contentID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var votes []*VoteInfo
	for rows.Next() {
		var vote VoteInfo
		var timestamp, lastUpdated int64
		var evidenceIDsJSON string

		err := rows.Scan(
			&vote.ContentID, &vote.VoterID, &vote.VoteType, &vote.VoteWeight,
			&vote.CertaintyLevel, &evidenceIDsJSON, &timestamp, &lastUpdated,
		)
		if err != nil {
			return nil, err
		}

		vote.Timestamp = time.Unix(timestamp, 0)
		vote.LastUpdated = time.Unix(lastUpdated, 0)
		if err := json.Unmarshal([]byte(evidenceIDsJSON), &vote.EvidenceIDs); err != nil {
			return nil, err
		}

		votes = append(votes, &vote)
	}

	return votes, rows.Err()
}

// Error Logging Operations

func (m *DBManager) LogError(err *models.AlgorithmError) error {
	contextJSON, jsonErr := json.Marshal(err.Context)
	if jsonErr != nil {
		return fmt.Errorf("failed to marshal error context: %v", jsonErr)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, dbErr := tx.Exec(`
			INSERT INTO algorithm_errors (
				algorithm_name, error_type, error_message,
				context, timestamp, resolved, resolution_notes
			) VALUES (?, ?, ?, ?, ?, ?, ?)`,
			err.AlgorithmName, err.ErrorType, err.ErrorMessage,
			string(contextJSON), err.Timestamp.Unix(), err.Resolved, err.ResolutionNotes,
		)
		return dbErr
	})
}

// Metrics Operations

func (m *DBManager) RecordMetric(metric *models.AlgorithmMetric) error {
	metadataJSON, err := json.Marshal(metric.Metadata)
	if err != nil {
		return fmt.Errorf("failed to marshal metric metadata: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT INTO algorithm_metrics (
				algorithm_name, metric_name, value,
				timestamp, metadata
			) VALUES (?, ?, ?, ?, ?)`,
			metric.AlgorithmName, metric.MetricName, metric.Value,
			metric.Timestamp.Unix(), string(metadataJSON),
		)
		return err
	})
}

// LogMetric stores an algorithm metric in the database
func (m *DBManager) LogMetric(metric *models.AlgorithmMetric) error {
	metadata, err := json.Marshal(metric.Metadata)
	if err != nil {
		return fmt.Errorf("failed to marshal metadata: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT INTO algorithm_metrics (
				algorithm_name, metric_name, value, timestamp, metadata
			) VALUES (?, ?, ?, ?, ?)`,
			metric.AlgorithmName, metric.MetricName, metric.Value,
			metric.Timestamp.Unix(), string(metadata),
		)
		return err
	})
}

// User Activity Operations

func (m *DBManager) GetUserVotes(userID string, since time.Time) ([]*models.Vote, error) {
	rows, err := m.db.Query(`
		SELECT id, content_id, voter_id, vote_type, vote_weight,
			   evidence_ids, timestamp, last_updated, explanation, context_score
		FROM truth_consensus 
		WHERE voter_id = ? AND timestamp >= ?`,
		userID, since.Unix())
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var votes []*models.Vote
	for rows.Next() {
		var vote models.Vote
		var timestamp, lastUpdated int64
		var evidenceIDsJSON string

		err := rows.Scan(
			&vote.ID, &vote.ContentID, &vote.UserID, &vote.Type, &vote.Weight,
			&evidenceIDsJSON, &timestamp, &lastUpdated, &vote.Explanation, &vote.ContextScore,
		)
		if err != nil {
			return nil, err
		}

		vote.Timestamp = time.Unix(timestamp, 0)
		vote.LastUpdated = time.Unix(lastUpdated, 0)
		if err := json.Unmarshal([]byte(evidenceIDsJSON), &vote.EvidenceIDs); err != nil {
			return nil, err
		}

		votes = append(votes, &vote)
	}

	return votes, rows.Err()
}

func (m *DBManager) GetUserEvidence(userID string, since time.Time) ([]*models.Evidence, error) {
	rows, err := m.db.Query(`
		SELECT id, content_id, submitter_id, evidence_type, url,
			   text, media_hash, description, timestamp, verification_state,
			   quality_score, context_data, references
		FROM evidence 
		WHERE submitter_id = ? AND timestamp >= ?`,
		userID, since.Unix())
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var evidences []*models.Evidence
	for rows.Next() {
		var evidence models.Evidence
		var timestamp int64
		var contextDataJSON, referencesJSON string

		err := rows.Scan(
			&evidence.ID, &evidence.ContentID, &evidence.SubmitterID,
			&evidence.EvidenceType, &evidence.URL, &evidence.Text,
			&evidence.MediaHash, &evidence.Description, &timestamp,
			&evidence.VerificationState, &evidence.QualityScore,
			&contextDataJSON, &referencesJSON,
		)
		if err != nil {
			return nil, err
		}

		evidence.Timestamp = time.Unix(timestamp, 0)
		if err := json.Unmarshal([]byte(contextDataJSON), &evidence.ContextData); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(referencesJSON), &evidence.References); err != nil {
			return nil, err
		}

		evidences = append(evidences, &evidence)
	}

	return evidences, rows.Err()
}

func (m *DBManager) GetUserContent(userID string, since time.Time) ([]*models.Content, error) {
	rows, err := m.db.Query(`
		SELECT id, author_id, content_type, content_text, media_urls,
			   parent_id, timestamp, signature, hash, processing_status,
			   last_updated, metadata, truth_score, visibility_score,
			   evidence_chains, topics, entities, context_refs
		FROM content 
		WHERE author_id = ? AND timestamp >= ?`,
		userID, since.Unix())
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var contents []*models.Content
	for rows.Next() {
		var content models.Content
		var timestamp, lastUpdated int64
		var mediaURLsJSON, metadataJSON, evidenceChainsJSON string
		var topicsJSON, entitiesJSON, contextRefsJSON string

		err := rows.Scan(
			&content.ID, &content.AuthorID, &content.ContentType,
			&content.ContentText, &mediaURLsJSON, &content.ParentID,
			&timestamp, &content.Signature, &content.Hash,
			&content.ProcessingStatus, &lastUpdated, &metadataJSON,
			&content.TruthScore, &content.VisibilityScore,
			&evidenceChainsJSON, &topicsJSON, &entitiesJSON, &contextRefsJSON,
		)
		if err != nil {
			return nil, err
		}

		content.Timestamp = time.Unix(timestamp, 0)
		content.LastUpdated = time.Unix(lastUpdated, 0)

		if err := json.Unmarshal([]byte(mediaURLsJSON), &content.MediaURLs); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(metadataJSON), &content.Metadata); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(evidenceChainsJSON), &content.EvidenceChains); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(topicsJSON), &content.Topics); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(entitiesJSON), &content.Entities); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(contextRefsJSON), &content.ContextRefs); err != nil {
			return nil, err
		}

		contents = append(contents, &content)
	}

	return contents, rows.Err()
}

func (m *DBManager) UpdateUser(user *models.User) error {
	sessionData, err := user.MarshalSessionData()
	if err != nil {
		return fmt.Errorf("failed to marshal session data: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			UPDATE users SET
				authenticity_score = ?,
				reputation_score = ?,
				truth_accuracy = ?,
				evidence_quality = ?,
				engagement_quality = ?,
				community_score = ?,
				last_active = ?,
				session_data = ?,
				stake_amount = ?,
				stake_locked_until = ?,
				verification_level = ?,
				total_contributions = ?
			WHERE id = ?`,
			user.AuthenticityScore,
			user.ReputationScore,
			user.TruthAccuracy,
			user.EvidenceQuality,
			user.EngagementQuality,
			user.CommunityScore,
			user.LastActive.Unix(),
			sessionData,
			user.StakeAmount,
			user.StakeLockedUntil,
			user.VerificationLevel,
			user.TotalContributions,
			user.ID,
		)
		return err
	})
}

// GetUserActivities retrieves user activities since a given time
func (m *DBManager) GetUserActivities(userID string, since time.Time) ([]*models.UserActivity, error) {
	rows, err := m.db.Query(`
		SELECT id, user_id, activity_type, target_id, timestamp, impact_score, metadata
		FROM user_activity 
		WHERE user_id = ? AND timestamp >= ?`,
		userID, since.Unix())
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var activities []*models.UserActivity
	for rows.Next() {
		var activity models.UserActivity
		var timestamp int64
		var metadataJSON string

		err := rows.Scan(
			&activity.ID, &activity.UserID, &activity.ActivityType,
			&activity.TargetID, &timestamp, &activity.ImpactScore,
			&metadataJSON,
		)
		if err != nil {
			return nil, err
		}

		activity.Timestamp = time.Unix(timestamp, 0)
		if err := json.Unmarshal([]byte(metadataJSON), &activity.Metadata); err != nil {
			return nil, err
		}

		activities = append(activities, &activity)
	}

	return activities, rows.Err()
}

// GetUserVerifications retrieves verification records for a user
func (m *DBManager) GetUserVerifications(userID string) ([]*models.UserVerification, error) {
	rows, err := m.db.Query(`
		SELECT id, user_id, verification_type, status, verified_at,
			   verifier_id, proof_data, metadata
		FROM user_verification 
		WHERE user_id = ?`,
		userID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var verifications []*models.UserVerification
	for rows.Next() {
		var verification models.UserVerification
		var verifiedAt sql.NullInt64
		var proofDataJSON, metadataJSON string

		err := rows.Scan(
			&verification.ID, &verification.UserID,
			&verification.VerificationType, &verification.Status,
			&verifiedAt, &verification.VerifierID,
			&proofDataJSON, &metadataJSON,
		)
		if err != nil {
			return nil, err
		}

		if verifiedAt.Valid {
			t := time.Unix(verifiedAt.Int64, 0)
			verification.VerifiedAt = &t
		}

		if err := json.Unmarshal([]byte(proofDataJSON), &verification.ProofData); err != nil {
			return nil, err
		}
		if err := json.Unmarshal([]byte(metadataJSON), &verification.Metadata); err != nil {
			return nil, err
		}

		verifications = append(verifications, &verification)
	}

	return verifications, rows.Err()
}

// CreateEvidence stores new evidence in the database
func (m *DBManager) CreateEvidence(evidence *models.Evidence) error {
	metadata, err := json.Marshal(evidence.Metadata)
	if err != nil {
		return fmt.Errorf("failed to marshal metadata: %v", err)
	}

	references, err := json.Marshal(evidence.References)
	if err != nil {
		return fmt.Errorf("failed to marshal references: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT INTO evidence (
				id, content_id, submitter_id, content_author_id,
				evidence_text, references, quality_score,
				timestamp, last_updated, metadata
			) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
			evidence.ID, evidence.ContentID, evidence.SubmitterID,
			evidence.ContentAuthorID, evidence.EvidenceText,
			string(references), evidence.QualityScore,
			evidence.Timestamp.Unix(), evidence.LastUpdated.Unix(),
			string(metadata),
		)
		return err
	})
}

// GetEvidence retrieves evidence by ID
func (m *DBManager) GetEvidence(id string) (*models.Evidence, error) {
	var evidence models.Evidence
	var refsJSON, metadataJSON string
	var timestamp, lastUpdated int64

	err := m.db.QueryRow(`
		SELECT id, content_id, submitter_id, content_author_id,
			   evidence_text, references, quality_score,
			   timestamp, last_updated, metadata
		FROM evidence WHERE id = ?`, id).Scan(
		&evidence.ID, &evidence.ContentID, &evidence.SubmitterID,
		&evidence.ContentAuthorID, &evidence.EvidenceText,
		&refsJSON, &evidence.QualityScore,
		&timestamp, &lastUpdated, &metadataJSON,
	)
	if err != nil {
		if err == sql.ErrNoRows {
			return nil, nil
		}
		return nil, err
	}

	evidence.Timestamp = time.Unix(timestamp, 0)
	evidence.LastUpdated = time.Unix(lastUpdated, 0)

	if err := json.Unmarshal([]byte(refsJSON), &evidence.References); err != nil {
		return nil, fmt.Errorf("failed to unmarshal references: %v", err)
	}

	if err := json.Unmarshal([]byte(metadataJSON), &evidence.Metadata); err != nil {
		return nil, fmt.Errorf("failed to unmarshal metadata: %v", err)
	}

	return &evidence, nil
}

// GetContentEvidence retrieves all evidence for a piece of content
func (m *DBManager) GetContentEvidence(contentID string) ([]*models.Evidence, error) {
	rows, err := m.db.Query(`
		SELECT id, content_id, submitter_id, content_author_id,
			   evidence_text, references, quality_score,
			   timestamp, last_updated, metadata
		FROM evidence WHERE content_id = ?
		ORDER BY quality_score DESC`, contentID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var evidence []*models.Evidence
	for rows.Next() {
		var e models.Evidence
		var refsJSON, metadataJSON string
		var timestamp, lastUpdated int64

		err := rows.Scan(
			&e.ID, &e.ContentID, &e.SubmitterID,
			&e.ContentAuthorID, &e.EvidenceText,
			&refsJSON, &e.QualityScore,
			&timestamp, &lastUpdated, &metadataJSON,
		)
		if err != nil {
			return nil, err
		}

		e.Timestamp = time.Unix(timestamp, 0)
		e.LastUpdated = time.Unix(lastUpdated, 0)

		if err := json.Unmarshal([]byte(refsJSON), &e.References); err != nil {
			return nil, fmt.Errorf("failed to unmarshal references: %v", err)
		}

		if err := json.Unmarshal([]byte(metadataJSON), &e.Metadata); err != nil {
			return nil, fmt.Errorf("failed to unmarshal metadata: %v", err)
		}

		evidence = append(evidence, &e)
	}

	return evidence, nil
}

// UpdateEvidence updates evidence in the database
func (m *DBManager) UpdateEvidence(evidence *models.Evidence) error {
	metadata, err := json.Marshal(evidence.Metadata)
	if err != nil {
		return fmt.Errorf("failed to marshal metadata: %v", err)
	}

	references, err := json.Marshal(evidence.References)
	if err != nil {
		return fmt.Errorf("failed to marshal references: %v", err)
	}

	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			UPDATE evidence SET
				evidence_text = ?,
				references = ?,
				quality_score = ?,
				last_updated = ?,
				metadata = ?
			WHERE id = ?`,
			evidence.EvidenceText,
			string(references),
			evidence.QualityScore,
			evidence.LastUpdated.Unix(),
			string(metadata),
			evidence.ID,
		)
		return err
	})
}

// Direct Message Operations

func (m *DBManager) CreateDirectMessage(message *models.DirectMessage) error {
	return m.Transaction(func(tx *sql.Tx) error {
		_, err := tx.Exec(`
			INSERT INTO direct_messages (
				id, sender_id, receiver_id, text, timestamp, read_at
			) VALUES (?, ?, ?, ?, ?, NULL)`,
			message.ID, message.SenderID, message.ReceiverID,
			message.Text, message.Timestamp.Unix(),
		)
		return err
	})
}

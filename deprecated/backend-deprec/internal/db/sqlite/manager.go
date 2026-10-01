package sqlite

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"log"
	"sync"
	"time"

	_ "github.com/mattn/go-sqlite3"
	"github.com/saint/babel-protocol/backend/api/models"
)

// DBManager handles database operations
type DBManager struct {
	db *sql.DB
	mu sync.Mutex
}

// NewDBManager creates a new database manager
func NewDBManager(dbPath string) (*DBManager, error) {
	db, err := sql.Open("sqlite3", dbPath)
	if err != nil {
		return nil, err
	}

	// Configure connection pool
	db.SetMaxOpenConns(25)
	db.SetMaxIdleConns(5)
	db.SetConnMaxLifetime(5 * time.Minute)

	return &DBManager{
		db: db,
	}, nil
}

// UpdateContentBatch updates multiple content entries in a single transaction
func (m *DBManager) UpdateContentBatch(contents []*models.Content) error {
	tx, err := m.db.Begin()
	if err != nil {
		return fmt.Errorf("failed to begin transaction: %v", err)
	}
	defer tx.Rollback()

	stmt, err := tx.Prepare(`
		UPDATE content SET 
			truth_score = ?,
			visibility_score = ?,
			processing_status = ?,
			last_updated = ?,
			topics = ?,
			entities = ?,
			consensus_state = ?,
			consensus_score = ?,
			consensus_validator_count = ?,
			consensus_temporal_weight = ?,
			metadata = ?
		WHERE id = ?
	`)
	if err != nil {
		return fmt.Errorf("failed to prepare statement: %v", err)
	}
	defer stmt.Close()

	for _, content := range contents {
		metadata, err := json.Marshal(content.Metadata)
		if err != nil {
			return fmt.Errorf("failed to marshal metadata: %v", err)
		}

		topics, err := json.Marshal(content.Topics)
		if err != nil {
			return fmt.Errorf("failed to marshal topics: %v", err)
		}

		entities, err := json.Marshal(content.Entities)
		if err != nil {
			return fmt.Errorf("failed to marshal entities: %v", err)
		}

		_, err = stmt.Exec(
			content.TruthScore,
			content.VisibilityScore,
			content.ProcessingStatus,
			content.LastUpdated.Unix(),
			string(topics),
			string(entities),
			content.Consensus.State,
			content.Consensus.Score,
			content.Consensus.ValidatorCount,
			content.Consensus.TemporalWeight,
			string(metadata),
			content.ID,
		)
		if err != nil {
			return fmt.Errorf("failed to update content %s: %v", content.ID, err)
		}
	}

	if err := tx.Commit(); err != nil {
		return fmt.Errorf("failed to commit transaction: %v", err)
	}

	return nil
}

// Close closes the database connection
func (m *DBManager) Close() error {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.db.Close()
}

// Transaction executes a function within a database transaction
func (m *DBManager) Transaction(fn func(*sql.Tx) error) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	tx, err := m.db.Begin()
	if err != nil {
		return fmt.Errorf("failed to begin transaction: %v", err)
	}

	if err := fn(tx); err != nil {
		if rbErr := tx.Rollback(); rbErr != nil {
			log.Printf("failed to rollback transaction: %v", rbErr)
		}
		return err
	}

	if err := tx.Commit(); err != nil {
		return fmt.Errorf("failed to commit transaction: %v", err)
	}

	return nil
}

// Helper functions for JSON handling

func jsonArrayToString(arr []string) (string, error) {
	if arr == nil {
		return "[]", nil
	}
	bytes, err := json.Marshal(arr)
	if err != nil {
		return "", fmt.Errorf("failed to marshal JSON array: %v", err)
	}
	return string(bytes), nil
}

func stringToJSONArray(s string) ([]string, error) {
	if s == "" {
		return []string{}, nil
	}
	var arr []string
	if err := json.Unmarshal([]byte(s), &arr); err != nil {
		return nil, fmt.Errorf("failed to unmarshal JSON array: %v", err)
	}
	return arr, nil
}

// Maintenance performs routine database maintenance
func (m *DBManager) Maintenance() error {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Run VACUUM to reclaim space and defragment
	_, err := m.db.Exec("VACUUM;")
	if err != nil {
		return fmt.Errorf("failed to vacuum database: %v", err)
	}

	// Analyze tables for query optimization
	_, err = m.db.Exec("ANALYZE;")
	if err != nil {
		return fmt.Errorf("failed to analyze database: %v", err)
	}

	// Clean expired cache entries
	_, err = m.db.Exec(`
		DELETE FROM algorithm_cache 
		WHERE expiry < ?;
	`, time.Now().Unix())
	if err != nil {
		return fmt.Errorf("failed to clean cache: %v", err)
	}

	return nil
}

// GetDB returns the underlying database connection
// Use with caution and prefer using the DBManager methods
func (m *DBManager) GetDB() *sql.DB {
	return m.db
}

// GetUserContexts retrieves a user's context posts since a given time
func (m *DBManager) GetUserContexts(userID string, since time.Time) ([]*models.Content, error) {
	rows, err := m.db.Query(`
		SELECT id, author_id, content_type, content_text, media_urls, truth_score, 
			   visibility_score, timestamp, last_updated, metadata, parent_id
		FROM content 
		WHERE author_id = ? AND content_type = 'context' AND timestamp > ?
		ORDER BY timestamp DESC
	`, userID, since.Unix())
	if err != nil {
		return nil, fmt.Errorf("failed to query user contexts: %v", err)
	}
	defer rows.Close()

	var contexts []*models.Content
	for rows.Next() {
		var c models.Content
		var mediaURLsJSON, metadataJSON []byte
		var parentID sql.NullString

		err := rows.Scan(&c.ID, &c.AuthorID, &c.ContentType, &c.ContentText, &mediaURLsJSON,
			&c.TruthScore, &c.VisibilityScore, &c.Timestamp, &c.LastUpdated, &metadataJSON, &parentID)
		if err != nil {
			return nil, fmt.Errorf("failed to scan context: %v", err)
		}

		// Unmarshal JSON fields
		if err := json.Unmarshal(mediaURLsJSON, &c.MediaURLs); err != nil {
			return nil, fmt.Errorf("failed to unmarshal media URLs: %v", err)
		}
		if err := json.Unmarshal(metadataJSON, &c.Metadata); err != nil {
			return nil, fmt.Errorf("failed to unmarshal metadata: %v", err)
		}
		if parentID.Valid {
			c.ParentID = &parentID.String
		}

		contexts = append(contexts, &c)
	}

	return contexts, nil
}

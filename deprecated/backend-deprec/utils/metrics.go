package utils

import (
	"encoding/json"
	"fmt"
	"os"
	"sync"
	"time"
)

// MetricType represents the type of metric
type MetricType string

const (
	Counter   MetricType = "counter"
	Gauge     MetricType = "gauge"
	Histogram MetricType = "histogram"
)

// Metric represents a single metric
type Metric struct {
	Name        string      `json:"name"`
	Type        MetricType  `json:"type"`
	Value       float64     `json:"value"`
	Labels      []string    `json:"labels,omitempty"`
	LastUpdated time.Time   `json:"last_updated"`
	History     []DataPoint `json:"history,omitempty"`
}

// DataPoint represents a single point in time for a metric
type DataPoint struct {
	Timestamp time.Time `json:"timestamp"`
	Value     float64   `json:"value"`
}

// Metrics handles all platform metrics
type Metrics struct {
	mu      sync.RWMutex
	metrics map[string]*Metric
	logger  *Logger
}

// NewMetrics creates a new metrics instance
func NewMetrics(logger *Logger) *Metrics {
	return &Metrics{
		metrics: make(map[string]*Metric),
		logger:  logger,
	}
}

// Platform-specific metrics tracking

// User Metrics
func (m *Metrics) TrackUserRegistration() {
	m.Increment("user_registrations_total")
}

func (m *Metrics) TrackUserActivity(userID string) {
	m.Increment("user_activity_total")
	m.Set("active_users", float64(m.countActiveUsers()))
}

// Content Metrics
func (m *Metrics) TrackContentCreation(contentType string) {
	m.Increment(fmt.Sprintf("content_created_total{type=%s}", contentType))
}

func (m *Metrics) TrackContentEngagement(contentID string, engagementType string) {
	m.Increment(fmt.Sprintf("content_engagements_total{type=%s}", engagementType))
}

// Truth Consensus Metrics
func (m *Metrics) TrackTruthScore(contentID string, score float64) {
	m.Set(fmt.Sprintf("content_truth_score{id=%s}", contentID), score)
	m.AddHistogramValue("truth_scores", score)
}

func (m *Metrics) TrackConsensusLevel(contentID string, level float64) {
	m.Set(fmt.Sprintf("content_consensus_level{id=%s}", contentID), level)
	m.AddHistogramValue("consensus_levels", level)
}

// Evidence Metrics
func (m *Metrics) TrackEvidenceSubmission(evidenceType string) {
	m.Increment(fmt.Sprintf("evidence_submitted_total{type=%s}", evidenceType))
}

func (m *Metrics) TrackEvidenceQuality(evidenceID string, score float64) {
	m.Set(fmt.Sprintf("evidence_quality_score{id=%s}", evidenceID), score)
	m.AddHistogramValue("evidence_quality_scores", score)
}

// System Metrics
func (m *Metrics) TrackAPILatency(path string, duration time.Duration) {
	m.AddHistogramValue(fmt.Sprintf("api_latency{path=%s}", path), duration.Seconds())
}

func (m *Metrics) TrackDBLatency(operation string, duration time.Duration) {
	m.AddHistogramValue(fmt.Sprintf("db_latency{operation=%s}", operation), duration.Seconds())
}

func (m *Metrics) TrackCacheHitRate(hit bool) {
	if hit {
		m.Increment("cache_hits_total")
	} else {
		m.Increment("cache_misses_total")
	}
}

// Core metric operations

func (m *Metrics) Increment(name string) {
	m.mu.Lock()
	defer m.mu.Unlock()

	metric, exists := m.metrics[name]
	if !exists {
		metric = &Metric{
			Name:    name,
			Type:    Counter,
			Value:   0,
			History: make([]DataPoint, 0),
		}
		m.metrics[name] = metric
	}

	metric.Value++
	metric.LastUpdated = time.Now()
	metric.History = append(metric.History, DataPoint{
		Timestamp: metric.LastUpdated,
		Value:     metric.Value,
	})

	m.logger.Debug("metrics", fmt.Sprintf("Incremented %s to %v", name, metric.Value))
}

func (m *Metrics) Set(name string, value float64) {
	m.mu.Lock()
	defer m.mu.Unlock()

	metric, exists := m.metrics[name]
	if !exists {
		metric = &Metric{
			Name:    name,
			Type:    Gauge,
			History: make([]DataPoint, 0),
		}
		m.metrics[name] = metric
	}

	metric.Value = value
	metric.LastUpdated = time.Now()
	metric.History = append(metric.History, DataPoint{
		Timestamp: metric.LastUpdated,
		Value:     value,
	})

	m.logger.Debug("metrics", fmt.Sprintf("Set %s to %v", name, value))
}

func (m *Metrics) AddHistogramValue(name string, value float64) {
	m.mu.Lock()
	defer m.mu.Unlock()

	metric, exists := m.metrics[name]
	if !exists {
		metric = &Metric{
			Name:    name,
			Type:    Histogram,
			History: make([]DataPoint, 0),
		}
		m.metrics[name] = metric
	}

	metric.Value = value // Latest value
	metric.LastUpdated = time.Now()
	metric.History = append(metric.History, DataPoint{
		Timestamp: metric.LastUpdated,
		Value:     value,
	})

	m.logger.Debug("metrics", fmt.Sprintf("Added histogram value %v to %s", value, name))
}

// Helper functions

func (m *Metrics) countActiveUsers() int {
	// This would typically query the database or cache
	// For now, it's a placeholder
	return 0
}

// SaveMetrics saves all metrics to a JSON file
func (m *Metrics) SaveMetrics(filepath string) error {
	m.mu.RLock()
	defer m.mu.RUnlock()

	data, err := json.MarshalIndent(m.metrics, "", "  ")
	if err != nil {
		return fmt.Errorf("failed to marshal metrics: %v", err)
	}

	return os.WriteFile(filepath, data, 0644)
}

// LoadMetrics loads metrics from a JSON file
func (m *Metrics) LoadMetrics(filepath string) error {
	data, err := os.ReadFile(filepath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil // No metrics file yet
		}
		return fmt.Errorf("failed to read metrics file: %v", err)
	}

	m.mu.Lock()
	defer m.mu.Unlock()

	return json.Unmarshal(data, &m.metrics)
}

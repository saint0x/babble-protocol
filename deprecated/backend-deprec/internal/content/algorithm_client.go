package content

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"github.com/saint/babel-protocol/backend/api/models"
)

// AlgorithmClient handles communication with the algorithm service
type AlgorithmClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewAlgorithmClient creates a new algorithm service client
func NewAlgorithmClient(baseURL string) *AlgorithmClient {
	return &AlgorithmClient{
		baseURL: baseURL,
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

// AnalyzeContent sends content to the algorithm service for analysis
func (c *AlgorithmClient) AnalyzeContent(content *models.Content) (*models.AlgorithmResponse, error) {
	reqBody := &models.AlgorithmRequest{
		Type:      "content_analysis",
		ContentID: content.ID,
		UserID:    content.AuthorID,
		Parameters: map[string]interface{}{
			"content_text": content.ContentText,
			"media_urls":   content.MediaURLs,
			"content_type": content.ContentType,
			"timestamp":    content.Timestamp,
		},
		Timestamp: time.Now(),
	}

	jsonData, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %v", err)
	}

	resp, err := c.httpClient.Post(
		fmt.Sprintf("%s/analyze", c.baseURL),
		"application/json",
		bytes.NewBuffer(jsonData),
	)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("algorithm service returned status %d", resp.StatusCode)
	}

	var result models.AlgorithmResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %v", err)
	}

	return &result, nil
}

// ValidateEvidence sends evidence to the algorithm service for validation
func (c *AlgorithmClient) ValidateEvidence(evidence *models.Evidence) (*models.AlgorithmResponse, error) {
	reqBody := &models.AlgorithmRequest{
		Type:      "evidence_validation",
		ContentID: evidence.ContentID,
		UserID:    evidence.SubmitterID,
		Parameters: map[string]interface{}{
			"evidence_id":   evidence.ID,
			"evidence_type": evidence.EvidenceType,
			"url":           evidence.URL,
			"text":          evidence.Text,
			"media_hash":    evidence.MediaHash,
			"description":   evidence.Description,
			"context_data":  evidence.ContextData,
		},
		Timestamp: time.Now(),
	}

	jsonData, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %v", err)
	}

	resp, err := c.httpClient.Post(
		fmt.Sprintf("%s/validate", c.baseURL),
		"application/json",
		bytes.NewBuffer(jsonData),
	)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("algorithm service returned status %d", resp.StatusCode)
	}

	var result models.AlgorithmResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %v", err)
	}

	return &result, nil
}

// UpdateConsensus sends updated consensus data to the algorithm service
func (c *AlgorithmClient) UpdateConsensus(contentID string, votes []*models.Vote) (*models.AlgorithmResponse, error) {
	reqBody := &models.AlgorithmRequest{
		Type:      "consensus_update",
		ContentID: contentID,
		Parameters: map[string]interface{}{
			"votes": votes,
		},
		Timestamp: time.Now(),
	}

	jsonData, err := json.Marshal(reqBody)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %v", err)
	}

	resp, err := c.httpClient.Post(
		fmt.Sprintf("%s/consensus", c.baseURL),
		"application/json",
		bytes.NewBuffer(jsonData),
	)
	if err != nil {
		return nil, fmt.Errorf("failed to send request: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("algorithm service returned status %d", resp.StatusCode)
	}

	var result models.AlgorithmResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %v", err)
	}

	return &result, nil
}

// AnalyzeContentBatch sends a batch of content to the algorithm service for analysis
func (c *AlgorithmClient) AnalyzeContentBatch(req *models.AlgorithmRequest) (*models.AlgorithmResponse, error) {
	body, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %v", err)
	}

	resp, err := c.httpClient.Post(c.baseURL+"/analyze/batch", "application/json", bytes.NewReader(body))
	if err != nil {
		return nil, fmt.Errorf("failed to send batch request: %v", err)
	}
	defer resp.Body.Close()

	var result models.AlgorithmResponse
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("failed to decode response: %v", err)
	}

	return &result, nil
}

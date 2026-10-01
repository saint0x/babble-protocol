package user

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"time"

	"github.com/saint/babel-protocol/backend/api/models"
)

// AlgorithmClient handles communication with the algorithm service for user-related operations
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

// AnalyzeUserActivity sends user activity data to the algorithm service for analysis
func (c *AlgorithmClient) AnalyzeUserActivity(req *models.AlgorithmRequest) (*models.AlgorithmResponse, error) {
	jsonData, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %v", err)
	}

	resp, err := c.httpClient.Post(
		fmt.Sprintf("%s/analyze/user/activity", c.baseURL),
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

// AnalyzeUserAuthenticity sends user verification data to the algorithm service
func (c *AlgorithmClient) AnalyzeUserAuthenticity(req *models.AlgorithmRequest) (*models.AlgorithmResponse, error) {
	jsonData, err := json.Marshal(req)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal request: %v", err)
	}

	resp, err := c.httpClient.Post(
		fmt.Sprintf("%s/analyze/user/authenticity", c.baseURL),
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

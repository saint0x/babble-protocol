package config

import "time"

// RateLimitConfig defines the configuration for rate limiting
type RateLimitConfig struct {
	Enabled           bool          // Whether rate limiting is enabled
	RequestsPerSecond float64       // Number of requests allowed per second
	Burst             int           // Maximum burst size
	Window            time.Duration // Time window for rate limiting
	WhiteList         []string      // List of IPs that are exempt from rate limiting
}

// NewRateLimitConfig creates a new rate limit configuration with default values
func NewRateLimitConfig() *RateLimitConfig {
	return &RateLimitConfig{
		Enabled:           true,        // Enable rate limiting by default
		RequestsPerSecond: 10,          // 10 requests per second by default
		Burst:             20,          // Allow bursts of up to 20 requests
		Window:            time.Minute, // 1 minute window
		WhiteList:         []string{},  // Empty whitelist by default
	}
}

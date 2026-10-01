package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/joho/godotenv"
)

// Config holds all configuration for the application
type Config struct {
	Environment         string
	ServerAddress       string
	DatabasePath        string
	AlgorithmServiceURL string
	RateLimit           RateLimit
	Redis               RedisConfig
	JWT                 JWTConfig
}

// RateLimit holds rate limiting configuration
type RateLimit struct {
	Enabled   bool
	Requests  int
	Window    time.Duration
	WhiteList []string
}

// RedisConfig holds Redis configuration
type RedisConfig struct {
	Address  string
	Password string
	DB       int
}

// JWTConfig holds JWT configuration
type JWTConfig struct {
	Secret        string
	ExpiryMinutes int
}

// Load loads configuration from environment variables
func Load() (*Config, error) {
	// Load .env file if it exists
	godotenv.Load()

	cfg := &Config{
		Environment:         getEnv("ENVIRONMENT", "development"),
		ServerAddress:       getEnv("SERVER_ADDRESS", ":8080"),
		DatabasePath:        getEnv("DATABASE_PATH", "babel.db"),
		AlgorithmServiceURL: getEnv("ALGORITHM_SERVICE_URL", "http://localhost:8081"),
		RateLimit: RateLimit{
			Enabled:   getBoolEnv("RATE_LIMIT_ENABLED", true),
			Requests:  getIntEnv("RATE_LIMIT_REQUESTS", 100),
			Window:    getDurationEnv("RATE_LIMIT_WINDOW", 1*time.Minute),
			WhiteList: getStringSliceEnv("RATE_LIMIT_WHITELIST", []string{}),
		},
		Redis: RedisConfig{
			Address:  getEnv("REDIS_ADDRESS", "localhost:6379"),
			Password: getEnv("REDIS_PASSWORD", ""),
			DB:       getIntEnv("REDIS_DB", 0),
		},
		JWT: JWTConfig{
			Secret:        getEnv("JWT_SECRET", ""),
			ExpiryMinutes: getIntEnv("JWT_EXPIRY_MINUTES", 60),
		},
	}

	// Validate required configuration
	if cfg.JWT.Secret == "" {
		return nil, fmt.Errorf("JWT_SECRET is required")
	}

	return cfg, nil
}

// Helper functions to get environment variables with defaults

func getEnv(key, defaultValue string) string {
	if value, exists := os.LookupEnv(key); exists {
		return value
	}
	return defaultValue
}

func getBoolEnv(key string, defaultValue bool) bool {
	if value, exists := os.LookupEnv(key); exists {
		b, err := strconv.ParseBool(value)
		if err == nil {
			return b
		}
	}
	return defaultValue
}

func getIntEnv(key string, defaultValue int) int {
	if value, exists := os.LookupEnv(key); exists {
		if i, err := strconv.Atoi(value); err == nil {
			return i
		}
	}
	return defaultValue
}

func getDurationEnv(key string, defaultValue time.Duration) time.Duration {
	if value, exists := os.LookupEnv(key); exists {
		if d, err := time.ParseDuration(value); err == nil {
			return d
		}
	}
	return defaultValue
}

func getStringSliceEnv(key string, defaultValue []string) []string {
	if value, exists := os.LookupEnv(key); exists && value != "" {
		return split(value, ",")
	}
	return defaultValue
}

func split(s string, sep string) []string {
	if s == "" {
		return []string{}
	}
	return strings.Split(s, sep)
}

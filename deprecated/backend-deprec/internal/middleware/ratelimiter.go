package middleware

import (
	"net/http"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/saint/babel-protocol/backend/internal/config"
	"golang.org/x/time/rate"
)

// RateLimiter implements a token bucket rate limiter
type RateLimiter struct {
	mu       sync.RWMutex
	config   *config.RateLimitConfig
	limiters map[string]*rateLimiterInfo
}

type rateLimiterInfo struct {
	limiter    *rate.Limiter
	lastAccess time.Time
}

// NewRateLimiter creates a new rate limiter
func NewRateLimiter(config *config.RateLimitConfig) *RateLimiter {
	rl := &RateLimiter{
		config:   config,
		limiters: make(map[string]*rateLimiterInfo),
	}
	go rl.cleanup(time.Minute * 5)
	return rl
}

// getLimiter returns a rate limiter for the given key
func (rl *RateLimiter) getLimiter(ip string) *rateLimiterInfo {
	rl.mu.Lock()
	defer rl.mu.Unlock()

	info, exists := rl.limiters[ip]
	if !exists {
		limiter := rate.NewLimiter(rate.Limit(rl.config.RequestsPerSecond), rl.config.Burst)
		info = &rateLimiterInfo{
			limiter:    limiter,
			lastAccess: time.Now(),
		}
		rl.limiters[ip] = info
	} else {
		info.lastAccess = time.Now()
	}
	return info
}

// Middleware returns a Gin middleware that implements rate limiting
func (rl *RateLimiter) Middleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		// Skip rate limiting if disabled
		if !rl.config.Enabled {
			c.Next()
			return
		}

		// Get client IP
		clientIP := c.ClientIP()

		// Check whitelist
		for _, ip := range rl.config.WhiteList {
			if ip == clientIP {
				c.Next()
				return
			}
		}

		// Get limiter for this IP
		limiter := rl.getLimiter(clientIP)

		// Try to allow request
		if !limiter.limiter.Allow() {
			c.JSON(http.StatusTooManyRequests, gin.H{
				"error": "rate limit exceeded",
			})
			c.Abort()
			return
		}

		c.Next()
	}
}

// cleanup periodically removes old limiters
func (rl *RateLimiter) cleanup(cleanupInterval time.Duration) {
	ticker := time.NewTicker(cleanupInterval)
	go func() {
		for range ticker.C {
			rl.mu.Lock()
			for ip, info := range rl.limiters {
				if time.Since(info.lastAccess) > rl.config.Window*2 {
					delete(rl.limiters, ip)
				}
			}
			rl.mu.Unlock()
		}
	}()
}

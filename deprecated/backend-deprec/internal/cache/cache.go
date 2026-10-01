package cache

import (
	"sync"
	"time"
)

// Cache implements a thread-safe in-memory cache
type Cache struct {
	mu            sync.RWMutex
	userScores    map[string]*UserScoreCache
	contentScores map[string]*ContentScoreCache
	ttl           time.Duration
}

type UserScoreCache struct {
	Scores      map[string]float64
	LastUpdated time.Time
	Version     int64
}

type ContentScoreCache struct {
	TruthScore      float64
	VisibilityScore float64
	LastUpdated     time.Time
	Version         int64
}

// NewCache creates a new cache instance
func NewCache(ttl time.Duration) *Cache {
	cache := &Cache{
		userScores:    make(map[string]*UserScoreCache),
		contentScores: make(map[string]*ContentScoreCache),
		ttl:           ttl,
	}
	go cache.cleanup()
	return cache
}

// GetUserScores retrieves cached user scores
func (c *Cache) GetUserScores(userID string) (map[string]float64, bool) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if cache, exists := c.userScores[userID]; exists {
		if time.Since(cache.LastUpdated) < c.ttl {
			return cache.Scores, true
		}
	}
	return nil, false
}

// SetUserScores caches user scores
func (c *Cache) SetUserScores(userID string, scores map[string]float64) {
	c.mu.Lock()
	defer c.mu.Unlock()

	c.userScores[userID] = &UserScoreCache{
		Scores:      scores,
		LastUpdated: time.Now(),
		Version:     time.Now().UnixNano(),
	}
}

// GetContentScores retrieves cached content scores
func (c *Cache) GetContentScores(contentID string) (*ContentScoreCache, bool) {
	c.mu.RLock()
	defer c.mu.RUnlock()

	if cache, exists := c.contentScores[contentID]; exists {
		if time.Since(cache.LastUpdated) < c.ttl {
			return cache, true
		}
	}
	return nil, false
}

// SetContentScores caches content scores
func (c *Cache) SetContentScores(contentID string, truth, visibility float64) {
	c.mu.Lock()
	defer c.mu.Unlock()

	c.contentScores[contentID] = &ContentScoreCache{
		TruthScore:      truth,
		VisibilityScore: visibility,
		LastUpdated:     time.Now(),
		Version:         time.Now().UnixNano(),
	}
}

// Invalidate removes entries from cache
func (c *Cache) Invalidate(keys ...string) {
	c.mu.Lock()
	defer c.mu.Unlock()

	for _, key := range keys {
		delete(c.userScores, key)
		delete(c.contentScores, key)
	}
}

// cleanup periodically removes expired entries
func (c *Cache) cleanup() {
	ticker := time.NewTicker(c.ttl)
	for range ticker.C {
		c.mu.Lock()
		now := time.Now()
		for id, cache := range c.userScores {
			if now.Sub(cache.LastUpdated) > c.ttl {
				delete(c.userScores, id)
			}
		}
		for id, cache := range c.contentScores {
			if now.Sub(cache.LastUpdated) > c.ttl {
				delete(c.contentScores, id)
			}
		}
		c.mu.Unlock()
	}
}

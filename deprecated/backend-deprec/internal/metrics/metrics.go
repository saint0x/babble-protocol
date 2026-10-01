package metrics

import (
	"strconv"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/prometheus/client_golang/prometheus"
)

// Metrics holds all Prometheus metrics
type Metrics struct {
	requestCounter  *prometheus.CounterVec
	requestDuration *prometheus.HistogramVec
	requestInFlight *prometheus.GaugeVec
}

// NewMetrics creates and registers Prometheus metrics
func NewMetrics() *Metrics {
	m := &Metrics{
		requestCounter: prometheus.NewCounterVec(
			prometheus.CounterOpts{
				Name: "http_requests_total",
				Help: "Total number of HTTP requests",
			},
			[]string{"method", "path", "status"},
		),
		requestDuration: prometheus.NewHistogramVec(
			prometheus.HistogramOpts{
				Name:    "http_request_duration_seconds",
				Help:    "HTTP request duration in seconds",
				Buckets: []float64{.005, .01, .025, .05, .1, .25, .5, 1, 2.5, 5, 10},
			},
			[]string{"method", "path"},
		),
		requestInFlight: prometheus.NewGaugeVec(
			prometheus.GaugeOpts{
				Name: "http_requests_in_flight",
				Help: "Current number of HTTP requests being served",
			},
			[]string{"method", "path"},
		),
	}

	// Register metrics with Prometheus
	prometheus.MustRegister(m.requestCounter)
	prometheus.MustRegister(m.requestDuration)
	prometheus.MustRegister(m.requestInFlight)

	return m
}

// RequestMiddleware returns a Gin middleware that records request metrics
func (m *Metrics) RequestMiddleware() gin.HandlerFunc {
	return func(c *gin.Context) {
		start := time.Now()
		path := c.FullPath()
		method := c.Request.Method

		// Track in-flight requests
		m.requestInFlight.WithLabelValues(method, path).Inc()
		defer m.requestInFlight.WithLabelValues(method, path).Dec()

		// Process request
		c.Next()

		// Record metrics
		status := c.Writer.Status()
		duration := time.Since(start).Seconds()

		m.requestCounter.WithLabelValues(method, path, strconv.Itoa(status)).Inc()
		m.requestDuration.WithLabelValues(method, path).Observe(duration)
	}
}

// RecordError records an error metric
func (m *Metrics) RecordError(method, path string) {
	m.requestCounter.WithLabelValues(method, path, "error").Inc()
}

// RecordDuration records a duration metric
func (m *Metrics) RecordDuration(method, path string, duration time.Duration) {
	m.requestDuration.WithLabelValues(method, path).Observe(duration.Seconds())
}

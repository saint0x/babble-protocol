package model;

import java.util.ArrayList;
import java.util.List;

public class User {
    // Define properties of the user class
    private String userId;
    private String username;
    private int authenticityScore;
    private List<String> followers;

    // Constructor
    public User(String userId, String username) {
        this.userId = userId;
        this.username = username;
        this.authenticityScore = 0; // Initialize authenticity score to 0
        this.followers = new ArrayList<>(); // Initialize followers list
    }

    // Getters and setters
    public String getUserId() {
        return userId;
    }

    public void setUserId(String userId) {
        this.userId = userId;
    }

    public String getUsername() {
        return username;
    }

    public void setUsername(String username) {
        this.username = username;
    }

    public int getAuthenticityScore() {
        return authenticityScore;
    }

    public void setAuthenticityScore(int authenticityScore) {
        this.authenticityScore = authenticityScore;
    }

    public List<String> getFollowers() {
        return followers;
    }

    public void setFollowers(List<String> followers) {
        this.followers = followers;
    }

    // Method to add a follower
    public void addFollower(String followerUserId) {
        followers.add(followerUserId);
    }

    // Method to remove a follower
    public void removeFollower(String followerUserId) {
        followers.remove(followerUserId);
    }

    // Method to increase authenticity score
    public void increaseAuthenticityScore() {
        authenticityScore++;
    }

    // Method to decrease authenticity score
    public void decreaseAuthenticityScore() {
        if (authenticityScore > 0) {
            authenticityScore--;
        }
    }
}

package model;

import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.nio.charset.StandardCharsets;

public class Transaction {
  // Define properties of the transaction class
  private String transactionId;
  private String fromUserId;
  private String toUserId;
  private String content;
  private long timestamp;
  private String signature; // Adding signature property

  // Constructor
  public Transaction(String fromUserId, String toUserId, String content) {
    this.fromUserId = fromUserId;
    this.toUserId = toUserId;
    this.content = content;
    this.timestamp = System.currentTimeMillis(); // Set timestamp to current time
    this.transactionId = generateTransactionId(); // Generate transaction ID
  }

  // Getters and setters
  public String getTransactionId() {
    return transactionId;
  }

  public void setTransactionId(String transactionId) {
    this.transactionId = transactionId;
  }

  public String getFromUserId() {
    return fromUserId;
  }

  public void setFromUserId(String fromUserId) {
    this.fromUserId = fromUserId;
  }

  public String getToUserId() {
    return toUserId;
  }

  public void setToUserId(String toUserId) {
    this.toUserId = toUserId;
  }

  public String getContent() {
    return content;
  }

  public void setContent(String content) {
    this.content = content;
  }

  public long getTimestamp() {
    return timestamp;
  }

  public void setTimestamp(long timestamp) {
    this.timestamp = timestamp;
  }

  public String getSignature() { // Adding getter for signature
    return signature;
  }

  public void setSignature(String signature) { // Adding setter for signature
    this.signature = signature;
  }

  // Method to generate a unique transaction ID
  private String generateTransactionId() {
    try {
      // Concatenate sender ID, recipient ID, and timestamp
      String data = fromUserId + toUserId + timestamp;

      // Hash the concatenated data using SHA-256 algorithm
      MessageDigest digest = MessageDigest.getInstance("SHA-256");
      byte[] hashBytes = digest.digest(data.getBytes(StandardCharsets.UTF_8));

      // Convert byte array to hexadecimal string
      StringBuilder hexString = new StringBuilder();
      for (byte b : hashBytes) {
        String hex = Integer.toHexString(0xff & b);
        if (hex.length() == 1)
          hexString.append('0');
        hexString.append(hex);
      }

      // Return the hexadecimal string as the transaction ID
      return "TXN-" + hexString.toString();
    } catch (NoSuchAlgorithmException e) {
      // Handle exception if SHA-256 algorithm is not available
      e.printStackTrace();
      // Fallback to using a combination of sender, recipient, and timestamp
      return "TXN-" + fromUserId + "-" + toUserId + "-" + timestamp;
    }
  }
}

/**
 * @brief: ECE358 network utility functions 
 * @author: ECE358 Teaching Staff
 * @file: net_util.h 
 * @date: 2017-05-02
 */

#ifndef NET_UTIL_H_
#define NET_UTIL_H_

//includes
#include <netinet/in.h>

// defines
#define PORT_RANGE_LO 10000
#define PORT_RANGE_HI 11000

// functions
uint32_t getPublicIPAddr();
int mybind(int sockfd, struct sockaddr_in *addr); 
int pickServerIPAddr(struct in_addr *srv_ip);

#endif // ! NET_UTIL_H_

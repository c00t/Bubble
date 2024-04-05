#include <Eigen/Core>
#include <Eigen/Geometry>

#include <stdio.h>

int main(void) {
  // simple test
  Eigen::VectorXd variables(6);
  variables << 1.0, 2.0, 3.0, 0.0, 0.0, 0.0;
  return 0;
}
